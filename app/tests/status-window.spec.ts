import { execFileSync, spawn } from 'node:child_process';
import type { ChildProcess } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import net from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { _electron as electron, expect, test as base } from '@playwright/test';
import type { ElectronApplication, Page } from '@playwright/test';
import { agentSocketPath } from '../src/agent-socket';

const appDir = path.resolve(__dirname, '..');
const agentBinary = path.resolve(appDir, '..', 'target', 'release', 'construct-agent');
const electronBinary = path.join(appDir, 'node_modules', '.bin', 'electron');
const refreshDeadlineMs = 10_000;
const secondLaunchDeadlineMs = 10_000;

// Under a virtual display on a VPS the Chromium sandbox cannot start, and `--no-sandbox` turns it
// off for every process. `sandbox: true` on the window still keeps Node out of the renderer, which
// the test below checks; the OS-level sandbox is not exercised by this run.
const launchArgs = ['--no-sandbox'];

type Session = {
  readonly agent: ChildProcess;
  readonly appArgs: readonly string[];
  readonly env: Record<string, string>;
};

function stringEnv(extra: Record<string, string>): Record<string, string> {
  const merged: Record<string, string> = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (value !== undefined) {
      merged[key] = value;
    }
  }
  return { ...merged, ...extra };
}

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

// Reloads the page under a listener for policy violations: any `securitypolicyviolation` event
// of the page fails the test that raised it.
async function watchPolicyViolations(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const violations: string[] = [];
    Object.assign(window, { policyViolations: violations });
    document.addEventListener('securitypolicyviolation', (event) => {
      violations.push(`${event.violatedDirective} ${event.blockedURI}`);
    });
  });
  await page.reload();
}

async function expectNoPolicyViolations(page: Page): Promise<void> {
  expect(await page.evaluate(() => Reflect.get(window, 'policyViolations') as string[])).toEqual(
    [],
  );
}

const test = base.extend<{ session: Session; app: ElectronApplication; page: Page }>({
  // A real agent from this repo, with its socket in a private runtime directory.
  // Playwright wants an object pattern here, even when the fixture needs nothing.
  // eslint-disable-next-line no-empty-pattern
  session: async ({}, use) => {
    const runtimeDir = mkdtempSync(path.join(tmpdir(), 'cb-run-'));
    const userDataDir = mkdtempSync(path.join(tmpdir(), 'cb-data-'));
    // The agent reads its settings from the home directory: a private one keeps a model
    // configured on the machine running the test out of the page and its screenshots.
    const homeDir = mkdtempSync(path.join(tmpdir(), 'cb-home-'));
    const env = stringEnv({
      XDG_RUNTIME_DIR: runtimeDir,
      TMPDIR: runtimeDir,
      HOME: homeDir,
      XDG_CONFIG_HOME: path.join(homeDir, '.config'),
    });
    const agent = spawn(agentBinary, ['run'], { env, stdio: 'ignore' });
    try {
      await expect(() => {
        execFileSync(agentBinary, ['status'], { env, stdio: 'ignore' });
      }).toPass({ timeout: 10_000 });
      await use({
        agent,
        env,
        appArgs: [appDir, ...launchArgs, `--user-data-dir=${userDataDir}`],
      });
    } finally {
      agent.kill('SIGTERM');
      rmSync(runtimeDir, { recursive: true, force: true });
      rmSync(userDataDir, { recursive: true, force: true });
      rmSync(homeDir, { recursive: true, force: true });
    }
  },
  app: async ({ session }, use) => {
    const app = await electron.launch({ args: [...session.appArgs], env: session.env });
    try {
      await use(app);
    } finally {
      await app.close();
    }
  },
  // The first window, watched for policy violations.
  page: async ({ app }, use) => {
    const page = await app.firstWindow();
    await watchPolicyViolations(page);
    await use(page);
    await expectNoPolicyViolations(page);
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
  const app = await electron.launch({ args: [...session.appArgs], env: session.env });
  try {
    const page = await app.firstWindow();
    await watchPolicyViolations(page);
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
  }));
  expect(inRenderer).toEqual({
    require: 'undefined',
    process: 'undefined',
    module: 'undefined',
    bridge: 'object',
    getStatus: 'function',
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

test('the sidebar switches between the Status and Settings pages', async ({ page }) => {
  const sidebar = page.getByRole('navigation', { name: 'Pages' });
  await expect(sidebar).toBeVisible();
  await expect(sidebar.locator('#sidebar-agent-state')).toBeVisible();
  expect((await sidebar.boundingBox())?.width).toBe(224);
  await expect(page.locator('#agent-status')).toHaveText('running');
  await expect(page.locator('#agent-detail')).toContainText(/^version .+, pid \d+$/);

  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  await expect(page.locator('#agent-status')).toHaveCount(0);

  await page.getByRole('button', { name: 'Status' }).click();
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
  // Neither `nativeTheme` nor `page.emulateMedia` changes the setting in an Electron window under
  // a virtual display; the DevTools protocol does.
  await expect(page.locator('#agent-status')).toHaveText('running');
  const cdp = await page.context().newCDPSession(page);
  const background = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  const backgrounds: string[] = [];
  for (const scheme of ['light', 'dark', 'light'] as const) {
    await cdp.send('Emulation.setEmulatedMedia', {
      features: [{ name: 'prefers-color-scheme', value: scheme }],
    });
    // The kit eases colours between themes; a screenshot taken mid-way shows a mix of both.
    await page.evaluate(() => Promise.all(document.getAnimations().map((a) => a.finished)));
    await page.screenshot({
      path: path.join(appDir, 'test-results', 'screenshots', `status-${scheme}.png`),
    });
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
