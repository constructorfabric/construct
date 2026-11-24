import { execFileSync } from 'node:child_process';
import { createHash, X509Certificate } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { createServer } from 'node:https';
import type { IncomingMessage, ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { gate } from './fixtures';

/** One request the test site received. */
export interface Hit {
  readonly path: string;
  readonly userAgent: string;
  readonly cookie: string | undefined;
  /** The `Sec-CH-UA` client hint of the request, when Chromium sent one. */
  readonly brands: string | undefined;
}

export interface Site {
  /** `https://127.0.0.1:<port>`, with no trailing slash. */
  readonly url: string;
  /** Every request, in order. */
  readonly hits: readonly Hit[];
  /** The Chromium switch that makes a test launcher accept this site's certificate, and only it. */
  readonly trustFlag: string;
  /** Lets the held request of `/slow` answer. */
  readonly releaseSlow: () => void;
  readonly close: () => Promise<void>;
}

const page = (body: string, head = ''): string =>
  `<!doctype html><meta charset="utf-8"><title>Test site</title>${head}<body>${body}</body>`;

const home = page(
  `<h1>Home</h1>
   <a id="blank" target="_blank" href="/second">second</a>
   <button id="open" onclick="window.open('/second')">open</button>
   <iframe src="/frame"></iframe>
   <script>
     fetch('/from-script');
     new Worker('/worker.js').onmessage = (event) => { window.workerAgent = event.data; };
   </script>`,
);

const counter = page(
  `<h1>Counter</h1>
   <button id="plus" onclick="document.getElementById('n').textContent = ++window.count">+</button>
   <span id="n">0</span>
   <script>window.count = 0;</script>`,
);

const pages: Record<string, string> = {
  '/': home,
  '/second': page('<h1>Second</h1>'),
  '/third': page('<h1>Third</h1>'),
  '/frame': page('<h1>Frame</h1><script>fetch("/from-frame")</script>'),
  '/counter': counter,
  '/editor': page(
    `<textarea id="source">secret text</textarea><textarea id="target"></textarea>
     <button id="print" onclick="window.print()">print</button>`,
  ),
  '/echo': page('<h1>Echo</h1>'),
  '/other': page('<h1>Other</h1>'),
};

const worker = `fetch('/from-worker').then(() => postMessage(navigator.userAgent));`;

/** An HTTPS site on this machine with a certificate no system trusts, and a log of what it was asked. */
export function startSite(): Promise<Site> {
  const dir = mkdtempSync(path.join(tmpdir(), 'cb-site-'));
  const keyFile = path.join(dir, 'key.pem');
  const certFile = path.join(dir, 'cert.pem');
  execFileSync(
    'openssl',
    [
      ...['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '2'],
      ...['-keyout', keyFile, '-out', certFile, '-subj', '/CN=127.0.0.1'],
      ...['-addext', 'subjectAltName=IP:127.0.0.1'],
    ],
    { stdio: 'ignore' },
  );
  const cert = readFileSync(certFile);
  const spki = new X509Certificate(cert).publicKey.export({ type: 'spki', format: 'der' });
  const hash = createHash('sha256').update(spki).digest('base64');
  const hits: Hit[] = [];
  const slow = gate();

  const answer = (request: IncomingMessage, response: ServerResponse): void => {
    const target = request.url ?? '/';
    const route = target.split('?')[0] ?? '/';
    hits.push({
      path: target,
      userAgent: request.headers['user-agent'] ?? '',
      cookie: request.headers.cookie,
      brands: String(request.headers['sec-ch-ua'] ?? '') || undefined,
    });
    const send = (type: string, body: string, headers: Record<string, string> = {}): void => {
      response.writeHead(200, { 'content-type': type, ...headers });
      response.end(body);
    };
    if (route === '/worker.js') {
      send('text/javascript', worker);
    } else if (route === '/redirect') {
      response.writeHead(302, { location: '/second' });
      response.end();
    } else if (route === '/flaky') {
      // The first request is cut off, as a failing network does; the next one is answered.
      if (hits.filter((hit) => hit.path === target).length === 1) {
        request.socket.destroy();
      } else {
        send('text/html', page('<h1>Flaky</h1>'));
      }
    } else if (route.startsWith('/to-')) {
      // `/to-mail`, `/to-file`: a redirect to an address that is not a web one.
      const targets: Record<string, string> = {
        '/to-mail': 'mailto:someone@example.com',
        '/to-file': 'file:///etc/hosts',
      };
      response.writeHead(302, { location: targets[route] ?? '/second' });
      response.end();
    } else if (route === '/cookie') {
      send('text/html', page('<h1>Cookie</h1>'), {
        'set-cookie': 'remembered=yes; Max-Age=3600; Secure; Path=/',
      });
    } else if (route === '/download') {
      send('text/plain', 'the file', {
        'content-disposition': 'attachment; filename="report.txt"',
      });
    } else if (route === '/slow') {
      void slow.promise.then(() => {
        send('text/plain', 'slow answer');
      });
    } else if (pages[route] !== undefined) {
      send('text/html', pages[route]);
    } else {
      response.writeHead(404);
      response.end();
    }
  };

  const server = createServer({ key: readFileSync(keyFile), cert }, answer);
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      resolve({
        url: `https://127.0.0.1:${(server.address() as AddressInfo).port}`,
        hits,
        trustFlag: `--ignore-certificate-errors-spki-list=${hash}`,
        releaseSlow: slow.release,
        close: () =>
          new Promise<void>((done) => {
            slow.release();
            server.closeAllConnections();
            server.close(() => {
              rmSync(dir, { recursive: true, force: true });
              done();
            });
          }),
      });
    });
  });
}
