import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import net from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';

/**
 * A fake agent on the socket of a new runtime directory. It records the line of each call and
 * answers with `answer(line, client)` once the answer is there. A `null` answer holds the reply back.
 * `close` stops it and removes the directory.
 */
export async function startFakeAgent(
  answer: (line: string, client: net.Socket) => string | null | Promise<string | null>,
  runtimeDir = mkdtempSync(path.join(tmpdir(), 'cb-unit-')),
): Promise<{
  readonly runtimeDir: string;
  readonly lines: readonly string[];
  readonly close: () => Promise<void>;
}> {
  mkdirSync(path.join(runtimeDir, 'construct'), { recursive: true });
  const lines: string[] = [];
  const clients = new Set<net.Socket>();
  const server = net.createServer((client) => {
    clients.add(client);
    client.setEncoding('utf8');
    client.once('data', (data: string) => {
      const line = data.trim();
      lines.push(line);
      void Promise.resolve(answer(line, client)).then((reply) => {
        if (reply !== null) {
          client.end(`${reply}\n`);
        }
      });
    });
    client.on('error', () => undefined);
  });
  await new Promise<void>((resolve) => {
    server.listen(path.join(runtimeDir, 'construct', 'agent.sock'), resolve);
  });
  return {
    runtimeDir,
    lines,
    close: async () => {
      clients.forEach((client) => {
        client.destroy();
      });
      await new Promise((resolve) => server.close(resolve));
      rmSync(runtimeDir, { recursive: true, force: true });
    },
  };
}
