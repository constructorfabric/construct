import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import net from 'node:net';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import type { ElectronApplication, Page } from '@playwright/test';
import { expect, openApp, test as base } from './fixtures';
import { startSite } from './glasshouse-site';
import type { Site } from './glasshouse-site';

/** An app that a test opened: `close` ends it early (a restart); the fixture ends any that is still open. */
interface Launched {
  readonly app: ElectronApplication;
  readonly page: Page;
  readonly close: () => Promise<void>;
}

type Launch = (options?: { trust?: boolean; start?: string }) => Promise<Launched>;

const test = base.extend<{ site: Site; launch: Launch }>({
  // eslint-disable-next-line no-empty-pattern
  site: async ({}, use) => {
    const site = await startSite();
    try {
      await use(site);
    } finally {
      await site.close();
    }
  },
  // Opens the app the way a test does: the site's certificate accepted when `trust` (the default),
  // the start address from `start`. An empty variable means "no start address", so no test reaches
  // the built-in one on the internet, whatever the environment of the run says.
  launch: async ({ session, site }, use) => {
    const opened: ElectronApplication[] = [];
    try {
      await use(async ({ trust = true, start } = {}) => {
        const env = { ...session.env, CONSTRUCT_GLASSHOUSE_URL: start ?? '' };
        const appArgs = trust ? [...session.appArgs, site.trustFlag] : session.appArgs;
        const { app, page } = await openApp({ ...session, env, appArgs });
        opened.push(app);
        return { app, page, close: () => app.close() };
      });
    } finally {
      for (const app of opened) {
        await app.close().catch(() => undefined);
      }
    }
  },
});

const openGlasshouse = (page: Page) =>
  page.getByRole('button', { name: 'Glasshouse', exact: true }).click();
const addressBar = (page: Page) => page.getByLabel('Address');

async function go(page: Page, address: string): Promise<void> {
  await addressBar(page).fill(address);
  await addressBar(page).press('Enter');
}

/** Runs a script in the web page whose address starts with `base`, as a click would, and returns its result. */
async function inPage(app: ElectronApplication, base: string, script: string): Promise<unknown> {
  // The address bar shows an address as soon as it is asked for; the page may not be there yet.
  for (let attempt = 0; attempt < 50; attempt++) {
    const found = await app.evaluate(
      ({ webContents }, base) =>
        webContents.getAllWebContents().some((c) => c.getURL().startsWith(base)),
      base,
    );
    if (found) {
      break;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  return app.evaluate(
    ({ webContents }, { base, script }) => {
      const contents = webContents.getAllWebContents().find((c) => c.getURL().startsWith(base));
      if (contents === undefined) {
        throw new Error(`no page at ${base}`);
      }
      return contents.executeJavaScript(script, true) as Promise<unknown>;
    },
    { base, script },
  );
}

/** A port on this machine that nothing listens on. */
async function closedPort(): Promise<number> {
  const server = net.createServer();
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address() as AddressInfo;
  await new Promise((resolve) => server.close(resolve));
  return port;
}

const paths = (site: Site) => site.hits.map((hit) => hit.path);

test('every request of the page, and of what it opens, names Construct and keeps Chrome', async ({
  launch,
  site,
}) => {
  const { app, page } = await launch({ start: `${site.url}/` });
  const version = await app.evaluate(({ app }) => app.getVersion());
  await openGlasshouse(page);
  await expect
    .poll(() => paths(site))
    .toEqual(
      expect.arrayContaining([
        '/',
        '/from-script',
        '/frame',
        '/from-frame',
        '/worker.js',
        '/from-worker',
      ]),
    );
  expect(site.hits.length).toBeGreaterThanOrEqual(6);
  for (const hit of site.hits) {
    expect(hit.userAgent, hit.path).toContain(`Construct/${version}`);
    expect(hit.userAgent, hit.path).toMatch(/Chrome\/\d+/);
    expect(hit.userAgent, hit.path).not.toContain('Electron/');
    expect(hit.userAgent, hit.path).not.toContain('ConstructBrowser/');
  }
  // The page's own scripts and its worker read the same name.
  const seen = await inPage(
    app,
    site.url,
    `new Promise((resolve) => {
    const wait = () => (window.workerAgent ? resolve([navigator.userAgent, window.workerAgent]) : setTimeout(wait, 50));
    wait();
  })`,
  );
  for (const agent of seen as string[]) {
    expect(agent).toContain(`Construct/${version}`);
    expect(agent).not.toContain('Electron/');
  }
  // The client hints carry no `Construct` brand: the page's list has brands, and so has the
  // `Sec-CH-UA` header that Chromium sent, and neither names Construct.
  const brands = (await inPage(
    app,
    site.url,
    'JSON.stringify(navigator.userAgentData.brands.map((brand) => brand.brand))',
  )) as string;
  expect(JSON.parse(brands) as string[]).toContain('Chromium');
  expect(brands).not.toContain('Construct');
  const sent = site.hits.flatMap((hit) => (hit.brands === undefined ? [] : [hit.brands]));
  expect(sent.length).toBeGreaterThan(0);
  for (const header of sent) {
    expect(header).toContain('Chromium');
    expect(header).not.toContain('Construct');
  }
});

test('an address typed with or without a scheme opens, and the bar follows redirects, back, forward and reload', async ({
  launch,
  site,
}) => {
  const { page } = await launch();
  await openGlasshouse(page);
  await go(page, `${site.url}/`);
  await expect.poll(() => paths(site)).toContain('/');
  // No scheme: `127.0.0.1:<port>/third` becomes https.
  await go(page, `${site.url.replace('https://', '')}/third`);
  await expect.poll(() => paths(site)).toContain('/third');
  await expect(addressBar(page)).toHaveValue(`${site.url}/third`);

  await go(page, `${site.url}/redirect`);
  await expect(addressBar(page)).toHaveValue(`${site.url}/second`);

  await page.getByRole('button', { name: 'Back' }).click();
  await expect(addressBar(page)).toHaveValue(`${site.url}/third`);
  await page.getByRole('button', { name: 'Forward' }).click();
  await expect(addressBar(page)).toHaveValue(`${site.url}/second`);

  const before = paths(site).filter((p) => p === '/second').length;
  await page.getByRole('button', { name: 'Reload' }).click();
  await expect.poll(() => paths(site).filter((p) => p === '/second').length).toBe(before + 1);
});

test('a link with target=_blank and window.open open in the same page', async ({
  launch,
  site,
}) => {
  const { app, page } = await launch({ start: `${site.url}/` });
  await openGlasshouse(page);
  await expect.poll(() => paths(site)).toContain('/');
  await expect(addressBar(page)).toHaveValue(`${site.url}/`);
  await inPage(app, site.url, "document.getElementById('blank').click()");
  await expect(addressBar(page)).toHaveValue(`${site.url}/second`);

  await page.getByRole('button', { name: 'Back' }).click();
  await expect(addressBar(page)).toHaveValue(`${site.url}/`);
  await inPage(app, site.url, "document.getElementById('open').click()");
  await expect(addressBar(page)).toHaveValue(`${site.url}/second`);
  expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length)).toBe(1);
});

test('the page keeps its counter and a slow request while another page is open', async ({
  launch,
  site,
}) => {
  const { app, page } = await launch({ start: `${site.url}/counter` });
  await openGlasshouse(page);
  await expect.poll(() => paths(site)).toContain('/counter');
  await expect(addressBar(page)).toHaveValue(`${site.url}/counter`);
  for (let i = 0; i < 3; i++) {
    await inPage(app, site.url, "document.getElementById('plus').click()");
  }
  await inPage(
    app,
    site.url,
    "window.slow = 'waiting'; fetch('/slow').then((r) => r.text()).then((t) => { window.slow = t; }); 1",
  );
  await expect.poll(() => paths(site)).toContain('/slow');

  await page.getByRole('button', { name: 'Chat', exact: true }).click();
  await expect(page.locator('#chat-input')).toBeVisible();
  const visible = () =>
    app.evaluate(({ BrowserWindow }) =>
      BrowserWindow.getAllWindows()[0]?.contentView.children.some((view) => view.getVisible()),
    );
  expect(await visible()).toBe(false);
  site.releaseSlow();
  await page.waitForTimeout(500);

  await openGlasshouse(page);
  await expect.poll(visible).toBe(true);
  expect(await inPage(app, site.url, "document.getElementById('n').textContent")).toBe('3');
  await expect.poll(() => inPage(app, site.url, 'window.slow')).toBe('slow answer');
  // It is the same page, not a new load of it.
  expect(paths(site).filter((p) => p === '/counter')).toHaveLength(1);
});

test('a cookie lives on after a restart, for the page only', async ({ launch, site }) => {
  const first = await launch({ start: `${site.url}/cookie` });
  await openGlasshouse(first.page);
  await expect.poll(() => paths(site)).toContain('/cookie');
  const cookies = (name: string) =>
    first.app.evaluate(async ({ session }, name) => {
      const ses = name === '' ? session.defaultSession : session.fromPartition(name);
      return (await ses.cookies.get({})).map((cookie) => cookie.name);
    }, name);
  await expect.poll(() => cookies('persist:glasshouse')).toEqual(['remembered']);
  expect(await cookies('')).toEqual([]);
  await first.app.evaluate(({ session }) =>
    session.fromPartition('persist:glasshouse').cookies.flushStore(),
  );

  await first.close();

  const second = await launch({ start: `${site.url}/echo` });
  await openGlasshouse(second.page);
  await expect.poll(() => paths(site)).toContain('/echo');
  expect(site.hits.find((hit) => hit.path === '/echo')?.cookie).toBe('remembered=yes');
});

test('the page finds nothing of the app, and permissions are refused without a prompt', async ({
  launch,
  site,
}) => {
  const { app, page } = await launch({ start: `${site.url}/` });
  await openGlasshouse(page);
  await expect.poll(() => paths(site)).toContain('/');
  await expect(addressBar(page)).toHaveValue(`${site.url}/`);
  expect(
    await inPage(
      app,
      site.url,
      '[typeof window.constructAgent, typeof require, typeof process, typeof module].join()',
    ),
  ).toBe('undefined,undefined,undefined,undefined');
  expect(await inPage(app, site.url, 'Notification.requestPermission()')).toBe('denied');
  expect(
    await inPage(
      app,
      site.url,
      `new Promise((resolve) =>
      navigator.geolocation.getCurrentPosition(() => resolve('granted'), (error) => resolve('denied ' + error.code)))`,
    ),
  ).toBe('denied 1');
  // The page is told "denied" for each of them: it has no prompt to wait for.
  expect(
    await inPage(
      app,
      site.url,
      `Promise.all(['camera', 'microphone', 'geolocation', 'notifications'].map(
        (name) => navigator.permissions.query({ name }).then((status) => status.state)))`,
    ),
  ).toEqual(['denied', 'denied', 'denied', 'denied']);
});

test('a certificate that is not trusted is refused and shown with Reload', async ({
  launch,
  site,
}) => {
  const { app, page } = await launch({ trust: false });
  await openGlasshouse(page);
  await go(page, `${site.url}/`);
  await expect(page.locator('#glasshouse-problem')).toContainText('certificate is not trusted');
  await expect(
    page.locator('#glasshouse-problem').getByRole('button', { name: 'Reload' }),
  ).toBeVisible();
  // Count the loads that fail from here on: Reload is shown to ask again by a second failure.
  await app.evaluate(({ webContents }) => {
    const failures: number[] = [];
    Object.assign(globalThis, { failures });
    webContents
      .getAllWebContents()
      .find((c) => !c.getURL().startsWith('file:'))
      ?.on('did-fail-load', (_event, code, _text, _url, isMainFrame) => {
        if (isMainFrame) {
          failures.push(code);
        }
      });
  });
  await page.locator('#glasshouse-problem').getByRole('button', { name: 'Reload' }).click();
  await expect
    .poll(() => app.evaluate(() => Reflect.get(globalThis, 'failures') as number[]))
    .toEqual([-202]);
  await expect(page.locator('#glasshouse-problem')).toContainText('certificate is not trusted');
  // The handshake failed each time: the site never saw a request.
  expect(site.hits).toEqual([]);
});

test('an address that cannot be reached says why, and Reload tries it again', async ({
  launch,
  site,
}) => {
  const { page } = await launch();
  const problem = page.locator('#glasshouse-problem');
  await openGlasshouse(page);

  // Nothing listens on this port: the connection is refused.
  const closed = await closedPort();
  await go(page, `https://127.0.0.1:${closed}/`);
  await expect(problem).toContainText('the site refused the connection');
  // A name that does not exist.
  await go(page, 'https://no-such-site.invalid/');
  await expect(problem).toContainText(/no site was found|no network connection/);

  // The first request of this address is cut off; Reload asks again, and the second is answered.
  await go(page, `${site.url}/flaky`);
  await expect(problem).toBeVisible();
  expect(paths(site).filter((p) => p === '/flaky')).toHaveLength(1);
  await problem.getByRole('button', { name: 'Reload' }).click();
  await expect(problem).toBeHidden();
  expect(paths(site).filter((p) => p === '/flaky')).toHaveLength(2);
  await expect(addressBar(page)).toHaveValue(`${site.url}/flaky`);
});

test('Back and Forward after a failed load clear the error and show the page', async ({
  launch,
  site,
}) => {
  const { page } = await launch();
  const problem = page.locator('#glasshouse-problem');
  await openGlasshouse(page);
  await go(page, `${site.url}/second`);
  await expect(addressBar(page)).toHaveValue(`${site.url}/second`);
  await expect.poll(() => paths(site)).toContain('/second');

  await go(page, `https://127.0.0.1:${await closedPort()}/`);
  await expect(problem).toBeVisible();
  await page.getByRole('button', { name: 'Back' }).click();
  await expect(problem).toBeHidden();
  await expect(addressBar(page)).toHaveValue(`${site.url}/second`);

  await page.getByRole('button', { name: 'Forward' }).click();
  await expect(problem).toBeVisible();
  await page.getByRole('button', { name: 'Back' }).click();
  await expect(problem).toBeHidden();
});

test('file: and javascript: addresses are refused in the bar', async ({ launch, site }) => {
  const { page } = await launch();
  await openGlasshouse(page);
  await go(page, `${site.url}/second`);
  await expect(addressBar(page)).toHaveValue(`${site.url}/second`);
  for (const address of ['file:///etc/hosts', 'javascript:document.title="x"']) {
    await go(page, address);
    await expect(page.locator('#glasshouse-refused')).toContainText('Only http and https');
  }
});

test('a page or a redirect cannot take the page to a non-web address, and a window cannot open one', async ({
  launch,
  site,
}) => {
  const { app, page } = await launch({ start: `${site.url}/second` });
  await openGlasshouse(page);
  await expect.poll(() => inPage(app, site.url, 'document.title')).toBe('Test site');
  const urls = () =>
    app.evaluate(({ webContents }) => webContents.getAllWebContents().map((c) => c.getURL()));
  const windows = () => app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length);
  // Listeners that run after the product's: `prevented` says what the product's guards did.
  await app.evaluate(({ webContents }, base) => {
    const seen: { event: string; url: string; prevented: boolean }[] = [];
    Object.assign(globalThis, { navigations: seen });
    const contents = webContents.getAllWebContents().find((c) => c.getURL().startsWith(base));
    const record =
      (name: string) =>
      (event: { url: string; defaultPrevented: boolean }): void => {
        seen.push({ event: name, url: event.url, prevented: event.defaultPrevented });
      };
    contents?.on('will-navigate', record('will-navigate'));
    contents?.on('will-redirect', record('will-redirect'));
  }, site.url);
  const navigations = () => app.evaluate(() => Reflect.get(globalThis, 'navigations') as unknown[]);

  // A script that sends the page to an address the system would hand to another program: the
  // product's guard stops it, and the page stays where it is.
  await inPage(app, site.url, "location.href = 'mailto:someone@example.com'; 1");
  await expect
    .poll(navigations)
    .toEqual([{ event: 'will-navigate', url: 'mailto:someone@example.com', prevented: true }]);
  await expect(addressBar(page)).toHaveValue(`${site.url}/second`);

  // A script that opens a file, a blank page or a mail address in a window: no window appears.
  await inPage(
    app,
    site.url,
    "window.open('file:///etc/hosts'); window.open('about:blank'); window.open('mailto:someone@example.com'); 1",
  );
  await page.waitForTimeout(500);
  expect(await windows()).toBe(1);
  expect(await urls()).not.toContainEqual(expect.stringMatching(/^file:\/\/\/etc/));
  await expect(addressBar(page)).toHaveValue(`${site.url}/second`);

  // A server that redirects to a mail address, which Chromium would hand to the system: the product's
  // guard stops it. A redirect to a file never gets as far as the guard: Chromium refuses it.
  for (const route of ['/to-mail', '/to-file']) {
    await go(page, `${site.url}${route}`);
    await expect.poll(() => paths(site)).toContain(route);
  }
  await expect.poll(navigations).toEqual([
    { event: 'will-navigate', url: 'mailto:someone@example.com', prevented: true },
    { event: 'will-redirect', url: 'mailto:someone@example.com', prevented: true },
  ]);
  expect(await urls()).not.toContainEqual(expect.stringMatching(/^(file:\/\/\/etc|mailto:)/));
});

test('a download leaves the choice of the file to a save dialog that starts from its name', async ({
  launch,
  site,
}) => {
  const { app, page } = await launch({ start: `${site.url}/second` });
  const dir = mkdtempSync(path.join(tmpdir(), 'cb-save-'));
  await openGlasshouse(page);
  await expect.poll(() => paths(site)).toContain('/second');
  const target = path.join(dir, 'chosen.txt');
  // The product sets no path, so Electron raises its one save dialog. A listener of the test, added
  // after the product's own, reads what that dialog would start from and answers it, as a person does.
  await app.evaluate(({ session }, target) => {
    const seen: { name: string; pathSet: boolean }[] = [];
    Object.assign(globalThis, { downloads: seen });
    session.fromPartition('persist:glasshouse').on('will-download', (_event, item) => {
      seen.push({
        name: item.getSaveDialogOptions().defaultPath ?? '',
        pathSet: item.getSavePath() !== '',
      });
      item.setSavePath(target);
    });
  }, target);
  await go(page, `${site.url}/download`);
  await expect.poll(() => existsSync(target) && readFileSync(target, 'utf8')).toBe('the file');
  expect(
    await app.evaluate(
      () => Reflect.get(globalThis, 'downloads') as { name: string; pathSet: boolean }[],
    ),
  ).toEqual([{ name: 'report.txt', pathSet: false }]);
  rmSync(dir, { recursive: true, force: true });
});

test('the start address comes from the environment; with an empty variable the page says so', async ({
  launch,
  site,
}) => {
  const withAddress = await launch({ start: `${site.url}/second` });
  await openGlasshouse(withAddress.page);
  await expect(addressBar(withAddress.page)).toHaveValue(`${site.url}/second`);
  await expect(withAddress.page.locator('#glasshouse-empty')).toBeHidden();
  await withAddress.close();

  const none = await launch();
  await openGlasshouse(none.page);
  await expect(none.page.locator('#glasshouse-empty')).toContainText('No start address is set');
  await go(none.page, `${site.url}/third`);
  await expect(addressBar(none.page)).toHaveValue(`${site.url}/third`);
  await expect(none.page.locator('#glasshouse-empty')).toBeHidden();
});

test('when the page process is killed the app goes on and Reload brings the page back', async ({
  launch,
  site,
}) => {
  const { app, page } = await launch({ start: `${site.url}/second` });
  await openGlasshouse(page);
  await expect(addressBar(page)).toHaveValue(`${site.url}/second`);
  // The address bar shows the address as soon as it is asked for: wait until the page has loaded.
  await expect.poll(() => inPage(app, site.url, 'document.title')).toBe('Test site');
  await app.evaluate(({ webContents }, base) => {
    const contents = webContents.getAllWebContents().find((c) => c.getURL().startsWith(base));
    if (contents === undefined) {
      throw new Error('no page to stop');
    }
    contents.forcefullyCrashRenderer();
  }, site.url);
  await expect(page.locator('#glasshouse-problem')).toContainText('stopped working');
  await page.getByRole('button', { name: 'Chat', exact: true }).click();
  await expect(page.locator('#chat-input')).toBeVisible();
  await openGlasshouse(page);
  await page.locator('#glasshouse-problem').getByRole('button', { name: 'Reload' }).click();
  await expect(page.locator('#glasshouse-problem')).toBeHidden();
  expect(await inPage(app, site.url, 'document.title')).toBe('Test site');
});
