import { execFileSync, spawn } from 'node:child_process';
import type { ChildProcess } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { _electron as electron, expect, test as base } from '@playwright/test';
import type { ElectronApplication } from '@playwright/test';

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

const test = base.extend<{ session: Session; app: ElectronApplication }>({
  // A real agent from this repo, with its socket in a private runtime directory.
  // Playwright wants an object pattern here, even when the fixture needs nothing.
  // eslint-disable-next-line no-empty-pattern
  session: async ({}, use) => {
    const runtimeDir = mkdtempSync(path.join(tmpdir(), 'cb-run-'));
    const userDataDir = mkdtempSync(path.join(tmpdir(), 'cb-data-'));
    const env = stringEnv({ XDG_RUNTIME_DIR: runtimeDir, TMPDIR: runtimeDir });
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
});

test('the window shows running, then stopped within 10 seconds of the agent stopping', async ({
  app,
  session,
}) => {
  const status = (await app.firstWindow()).locator('#agent-status');
  await expect(status).toHaveText('running');

  session.agent.kill('SIGTERM');
  await expect(status).toHaveText('stopped', { timeout: refreshDeadlineMs });
});

test('a second launch exits at once and opens no window of its own', async ({ app, session }) => {
  await expect((await app.firstWindow()).locator('#agent-status')).toHaveText('running');

  expect(await launchSecondInstance(session)).toEqual({ code: 0, signal: null });
});

test('a second launch restores and focuses the first window', async ({ app, session }) => {
  await expect((await app.firstWindow()).locator('#agent-status')).toHaveText('running');
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
}) => {
  const page = await app.firstWindow();
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

test('the window cannot navigate away or open another window', async ({ app }) => {
  const page = await app.firstWindow();
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
