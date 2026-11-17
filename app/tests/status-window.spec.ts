import { spawn } from 'node:child_process';
import { mkdirSync, rmSync } from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { agentSocketPath } from '../src/agent-socket';
import type { Page } from '@playwright/test';
import { appDir, expect, expectNoPolicyViolations, openApp, test as base } from './fixtures';
import type { Session } from './fixtures';
import { screenshotBothThemes, screenshotInTheme } from './screenshots';

const electronBinary = path.join(appDir, 'node_modules', '.bin', 'electron');
const refreshDeadlineMs = 10_000;
const secondLaunchDeadlineMs = 10_000;

type Exit = { readonly code: number | null; readonly signal: NodeJS.Signals | null };

// A second instance that did not get the lock never creates a window and quits;
// one that did would keep running, and the kill turns that into a failure.
async function launchSecondInstance(session: Session): Promise<Exit> {
  const second = spawn(electronBinary, [...session.appArgs], {
    env: session.env,
    stdio: 'ignore',
  });
  const killTimer = setTimeout(() => second.kill('SIGKILL'), secondLaunchDeadlineMs);
  const exit = await new Promise<Exit>((resolve) => {
    second.on('exit', (code, signal) => {
      resolve({ code, signal });
    });
  });
  clearTimeout(killTimer);
  return exit;
}

// Chat is the first page and the default; these tests are about the Status page.
const test = base.extend<{ page: Page }>({
  page: async ({ page }, use) => {
    await page.getByRole('button', { name: 'Status', exact: true }).click();
    await use(page);
  },
});

test('the window and its sidebar show running, then stopped within 10 seconds of the agent stopping', async ({
  page,
  session,
}) => {
  const status = page.locator('#agent-status');
  const sidebarState = page.locator('#sidebar-agent-state');
  await expect(status).toHaveText('running');
  await expect(sidebarState).toHaveAttribute('data-state', 'running');
  await expect(page.locator('#sidebar-agent-state-word')).toHaveText('running');

  session.agent.kill('SIGTERM');
  await expect(status).toHaveText('stopped', { timeout: refreshDeadlineMs });
  await expect(sidebarState).toHaveAttribute('data-state', 'stopped');
  await expect(page.locator('#sidebar-agent-state-word')).toHaveText('stopped');
});

test('the Status page shows the state word in a badge, the version and the model', async ({
  page,
}) => {
  const badge = page.locator('#agent-status');
  await expect(badge).toHaveText('running');
  await expect(badge).toHaveAttribute('data-state', 'running');
  await expect(page.locator('#agent-detail')).toContainText(/^version .+, pid \d+$/);
  await expect(page.locator('#agent-model')).toHaveText('model not set');
});

test('the Status page shows stopped, with its reason, when no agent runs', async ({
  page,
  session,
}) => {
  await expect(page.locator('#agent-status')).toHaveText('running');
  session.agent.kill('SIGTERM');

  const badge = page.locator('#agent-status');
  await expect(badge).toHaveText('stopped', { timeout: refreshDeadlineMs });
  await expect(badge).toHaveAttribute('data-state', 'stopped');
  await expect(page.locator('#agent-detail')).toContainText(/ENOENT|ECONNREFUSED/);
  await expect(page.locator('#agent-model')).toHaveCount(0);
});

test('the Status page shows unknown, with its reason, when the agent answers wrongly', async ({
  session,
}) => {
  // A stand-in that listens on the agent's socket and answers a line the app cannot read.
  const socketPath = agentSocketPath(session.env, process.platform);
  const socketDir = path.dirname(socketPath);
  session.agent.kill('SIGTERM');
  await new Promise((resolve) => session.agent.once('exit', resolve));
  mkdirSync(socketDir, { recursive: true });
  rmSync(socketPath, { force: true });
  const server = net.createServer((socket) => {
    socket.on('error', () => undefined);
    socket.end('{"status":"confused"}\n');
  });
  await new Promise<void>((resolve) => server.listen(socketPath, resolve));
  const { app, page } = await openApp(session);
  try {
    await page.getByRole('button', { name: 'Status', exact: true }).click();
    const badge = page.locator('#agent-status');
    await expect(badge).toHaveText('unknown');
    await expect(badge).toHaveAttribute('data-state', 'unknown');
    await expect(page.locator('#agent-detail')).toContainText('did not report running');
    await expectNoPolicyViolations(page);
  } finally {
    await app.close();
    server.close();
  }
});

test('a second launch exits at once and opens no window of its own', async ({ page, session }) => {
  await expect(page.locator('#agent-status')).toHaveText('running');

  expect(await launchSecondInstance(session)).toEqual({ code: 0, signal: null });
});

test('a second launch restores and focuses the first window', async ({ app, page, session }) => {
  await expect(page.locator('#agent-status')).toHaveText('running');
  // No window manager runs under the virtual display, so a window cannot really be
  // minimized or focused; record the calls the app makes on its window instead.
  await app.evaluate(({ BrowserWindow }) => {
    const calls: string[] = [];
    const window = BrowserWindow.getAllWindows()[0];
    if (window === undefined) {
      throw new Error('the app has no window');
    }
    window.isMinimized = () => true;
    window.restore = () => {
      calls.push('restore');
    };
    window.focus = () => {
      calls.push('focus');
    };
    Object.assign(globalThis, { windowCalls: calls });
  });

  await launchSecondInstance(session);

  await expect
    .poll(() => app.evaluate(() => Reflect.get(globalThis, 'windowCalls') as string[]))
    .toEqual(['restore', 'focus']);
});

test('the window runs with the isolation settings and the bridge, and without Node', async ({
  app,
  page,
}) => {
  await expect(page.locator('#agent-status')).toHaveText('running');

  // `getLastWebPreferences` is real but missing from Electron's typings, so it is reached by name.
  const preferences = await app.evaluate(({ BrowserWindow }) => {
    const contents = BrowserWindow.getAllWindows()[0]?.webContents;
    const last: unknown = Reflect.apply(
      Reflect.get(contents ?? {}, 'getLastWebPreferences'),
      contents,
      [],
    );
    return {
      contextIsolation: Reflect.get(Object(last), 'contextIsolation'),
      nodeIntegration: Reflect.get(Object(last), 'nodeIntegration'),
      sandbox: Reflect.get(Object(last), 'sandbox'),
    };
  });
  expect(preferences).toEqual({ contextIsolation: true, nodeIntegration: false, sandbox: true });

  const inRenderer = await page.evaluate(() => ({
    require: typeof require,
    process: typeof process,
    module: typeof module,
    bridge: typeof window.constructAgent,
    getStatus: typeof window.constructAgent.getStatus,
    getNeedsMove: typeof window.constructAgent.getNeedsMove,
  }));
  expect(inRenderer).toEqual({
    require: 'undefined',
    process: 'undefined',
    module: 'undefined',
    bridge: 'object',
    getStatus: 'function',
    getNeedsMove: 'function',
  });
});

test('the window cannot navigate away or open another window', async ({ app, page }) => {
  await expect(page.locator('#agent-status')).toHaveText('running');
  // The app's own handler is registered first, so by the time this one runs the navigation
  // has been prevented or not.
  await app.evaluate(({ BrowserWindow }) => {
    const prevented: boolean[] = [];
    BrowserWindow.getAllWindows()[0]?.webContents.on('will-navigate', (event) => {
      prevented.push(event.defaultPrevented);
    });
    Object.assign(globalThis, { prevented });
  });

  await page.evaluate(() => {
    window.open('http://127.0.0.1:1/');
    window.location.href = 'http://127.0.0.1:1/';
  });

  await expect
    .poll(() => app.evaluate(() => Reflect.get(globalThis, 'prevented') as boolean[]))
    .toEqual([true]);
  // Read the page directly: Playwright's locators wait on the navigation that was refused.
  expect(page.url()).toMatch(/\/index\.html$/);
  expect(await page.evaluate(() => document.getElementById('agent-status')?.textContent)).toBe(
    'running',
  );
  expect(app.windows()).toHaveLength(1);
});

test('the sidebar switches between the Chat, Status and Settings pages', async ({ page }) => {
  const sidebar = page.getByRole('navigation', { name: 'Pages' });
  await expect(sidebar).toBeVisible();
  await expect(sidebar.locator('#sidebar-agent-state')).toBeVisible();
  expect((await sidebar.boundingBox())?.width).toBe(224);
  await expect(page.locator('#agent-status')).toHaveText('running');
  await expect(page.locator('#agent-detail')).toContainText(/^version .+, pid \d+$/);

  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible();
  await expect(page.locator('#agent-status')).toHaveCount(0);

  await page.getByRole('button', { name: 'Chat', exact: true }).click();
  await expect(page.locator('#chat-input')).toBeVisible();
  await expect(page.locator('#agent-status')).toHaveCount(0);

  await page.getByRole('button', { name: 'Status', exact: true }).click();
  await expect(page.locator('#agent-status')).toHaveText('running');
});

test('the window opens at 960 by 640 with a 720 by 480 minimum', async ({ app, page }) => {
  await expect(page.locator('#agent-status')).toHaveText('running');

  const sizes = await app.evaluate(({ BrowserWindow }) => {
    const window = BrowserWindow.getAllWindows()[0];
    return { size: window?.getSize(), minimum: window?.getMinimumSize() };
  });
  expect(sizes).toEqual({ size: [960, 640], minimum: [720, 480] });
});

test('the theme follows the operating system', async ({ page }) => {
  await expect(page.locator('#agent-status')).toHaveText('running');
  const cdp = await page.context().newCDPSession(page);
  const background = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  const backgrounds: string[] = [];
  for (const scheme of ['light', 'dark', 'light'] as const) {
    await screenshotInTheme(page, cdp, scheme, 'status');
    backgrounds.push(await background());
  }
  const [light, dark, lightAgain] = backgrounds;
  expect(dark).not.toBe(light);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollHeight <= document.documentElement.clientHeight,
    ),
  ).toBe(true);
  expect(lightAgain).toBe(light);
});

test('the Start at login block shows not registered when nothing registered the agent', async ({
  page,
}) => {
  const block = page.locator('#start-at-login');
  await expect(block).toHaveText('not registered');
  await expect(block).toHaveAttribute('data-state', 'not_registered');
  await expect(page.getByRole('heading', { name: 'Start at login' })).toBeVisible();

  await screenshotBothThemes(page, 'start-at-login-not-registered');
});

test('the window tells the user to move the app only when the app asks for it', async ({
  app,
  page,
}) => {
  await expect(page.locator('#agent-status')).toHaveText('running');
  await expect(page.locator('#needs-move')).toHaveCount(0);

  await app.evaluate(({ ipcMain }) => {
    ipcMain.removeHandler('agent:needs-move');
    ipcMain.handle('agent:needs-move', () => true);
  });
  await page.reload();

  // The notice is an alert for assistive technology, not only a paragraph with an id.
  const notice = page.getByRole('alert').filter({
    hasText:
      'Move Construct Browser to Applications, then reopen it, so the agent starts at login.',
  });
  await expect(notice).toHaveCount(1);
  await expect(notice).toHaveAttribute('id', 'needs-move');
  await screenshotBothThemes(page, 'needs-move');
});
