import { spawn } from 'node:child_process';
import path from 'node:path';
import type { Page } from '@playwright/test';
import { appDir, expect, test as base, withStandIn } from './fixtures';
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

function runningReply(fields: Record<string, unknown>): string {
  return JSON.stringify({
    status: 'running',
    version: '0.1.0',
    pid: 42,
    model: 'example-model',
    ...fields,
  });
}

test('the Status page shows unknown, with its reason, when the agent answers wrongly', async ({
  session,
}) => {
  // A stand-in that answers a line the app cannot read.
  await withStandIn(session, '{"status":"confused"}', async (page) => {
    await page.getByRole('button', { name: 'Status', exact: true }).click();
    const badge = page.locator('#agent-status');
    await expect(badge).toHaveText('unknown');
    await expect(badge).toHaveAttribute('data-state', 'unknown');
    await expect(page.locator('#agent-detail')).toContainText('did not report running');
  });
});

test('the Status page shows the database and the Construct gear of the real agent', async ({
  page,
}) => {
  // The first start unpacks PostgreSQL, which is longer than the default test timeout.
  test.setTimeout(150_000);
  await expect(page.locator('#agent-status')).toHaveText('running');
  await expect(page.getByRole('heading', { name: 'Database', exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Construct gear', exact: true })).toBeVisible();
  for (const part of ['database', 'construct']) {
    await expect(page.locator(`#${part}-state`)).toHaveAttribute(
      'data-state',
      /^(ready|starting|failed)$/,
    );
  }
  if (process.platform === 'darwin') {
    // The agent has no pgvector for macOS yet: neither the database nor the gear can run.
    await expect(page.locator('#database-state')).toHaveText('failed', {
      timeout: refreshDeadlineMs,
    });
    await expect(page.locator('#database-reason')).toHaveText(
      'pgvector is not built for this platform yet',
    );
    await expect(page.locator('#construct-state')).toHaveText('failed');
    await expect(page.locator('#construct-reason')).toHaveText('the database is not running');
  } else {
    // Only the database is waited for: the gear's own start is covered by the Rust tests.
    await expect(page.locator('#database-state')).toHaveText('ready', { timeout: 120_000 });
    await expect(page.locator('#database-reason')).toHaveCount(0);
  }
  // A reason never shows a path.
  for (const reason of await page.locator('[id$="-reason"]').allTextContents()) {
    expect(reason).not.toMatch(/[/\\]/);
  }
  await screenshotBothThemes(page, 'status-parts');
});

test('the Status page shows each part with its state and, when it is not ready, why', async ({
  session,
}) => {
  const reply = runningReply({
    database: { state: 'ready', reason: 'never shown' },
    construct: {
      state: 'failed',
      reason: 'the Construct gear did not start; the agent log has the details',
    },
  });
  await withStandIn(session, reply, async (page) => {
    await page.getByRole('button', { name: 'Status', exact: true }).click();
    await expect(page.locator('#database-state')).toHaveText('ready');
    await expect(page.locator('#database-state')).toHaveAttribute('data-state', 'ready');
    await expect(page.locator('#database-reason')).toHaveCount(0);
    await expect(page.locator('#construct-state')).toHaveText('failed');
    await expect(page.locator('#construct-state')).toHaveAttribute('data-state', 'failed');
    await expect(page.locator('#construct-reason')).toHaveText(
      'the Construct gear did not start; the agent log has the details',
    );
    await screenshotBothThemes(page, 'status-parts-construct-failed');
  });
});

test('the Status page reads the plain states of an older agent and skips what it does not know', async ({
  session,
}) => {
  const reply = runningReply({ database: 'starting', construct: { state: 'asleep' } });
  await withStandIn(session, reply, async (page) => {
    await page.getByRole('button', { name: 'Status', exact: true }).click();
    await expect(page.locator('#agent-status')).toHaveText('running');
    await expect(page.locator('#database-state')).toHaveText('starting');
    await expect(page.locator('#database-reason')).toHaveCount(0);
    await expect(page.getByRole('heading', { name: 'Construct gear' })).toHaveCount(0);
    await expect(page.locator('#construct-state')).toHaveCount(0);
    // An agent that does not tell the gear's state gives the Chat page no reason for a notice.
    await page.getByRole('button', { name: 'Chat', exact: true }).click();
    await expect(page.locator('#chat-input')).toBeVisible();
    await expect(page.locator('#chat-no-profile')).toHaveCount(0);
  });
});

test('the Chat page says in one line that the profile is not available while the gear is not ready', async ({
  session,
}) => {
  const line = 'Profile is not available: the Construct gear is not running';
  await withStandIn(
    session,
    runningReply({ construct: { state: 'starting', reason: 'waiting for the database' } }),
    async (page) => {
      await expect(page.locator('#chat-input')).toBeVisible();
      const notice = page.locator('#chat-no-profile');
      await expect(notice).toHaveText(line);
      await expect(notice).toHaveAttribute('role', 'status');
      await screenshotBothThemes(page, 'chat-no-profile');
    },
  );
  await session.startAgent();
  await withStandIn(session, runningReply({ construct: { state: 'ready' } }), async (page) => {
    await expect(page.locator('#chat-input')).toBeVisible();
    await expect(page.locator('#sidebar-agent-state')).toHaveAttribute('data-state', 'running');
    await expect(page.locator('#chat-no-profile')).toHaveCount(0);
  });
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

test('the sidebar switches between the Chat, Dashboard, Status and Settings pages', async ({
  page,
}) => {
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

  await page.getByRole('button', { name: 'Dashboard', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Dashboard', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Dashboard', exact: true })).toHaveAttribute(
    'aria-current',
    'page',
  );
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
  // The page may be taller than the window (the real agent adds a card for each tool it collects
  // from), so the check is that a theme does not change its height, not that it never scrolls.
  const height = () => page.evaluate(() => document.documentElement.scrollHeight);
  const backgrounds: string[] = [];
  const heights: number[] = [];
  for (const scheme of ['light', 'dark', 'light'] as const) {
    await screenshotInTheme(page, cdp, scheme, 'status');
    backgrounds.push(await background());
    heights.push(await height());
  }
  const [light, dark, lightAgain] = backgrounds;
  expect(dark).not.toBe(light);
  expect(new Set(heights).size).toBe(1);
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

function secondsAgo(seconds: number): string {
  return new Date(Date.now() - seconds * 1000).toISOString();
}

test('a turn stored a few seconds ago shows collecting in the sidebar and on the page', async ({
  session,
}) => {
  const reply = runningReply({
    connectors: {
      'claude-code': { turns: 12, skipped: 0, scanning: false, last_turn: secondsAgo(5) },
      codex: { turns: 0, skipped: 0, scanning: false },
    },
  });
  await withStandIn(session, reply, async (page) => {
    await page.getByRole('button', { name: 'Status', exact: true }).click();
    await expect(page.locator('#sidebar-collection')).toHaveText('collecting');
    const claude = page.locator('#connector-claude-code');
    await expect(claude.getByRole('heading', { name: 'Claude Code' })).toBeVisible();
    await expect(page.locator('#connector-claude-code-state')).toHaveText('collecting');
    await expect(page.locator('#connector-claude-code-turns')).toHaveText('12 turns stored');
    await expect(page.locator('#connector-claude-code-last-turn')).toHaveText('last turn just now');
    await expect(page.locator('#connector-claude-code-reason')).toHaveCount(0);
    await expect(page.locator('#connector-codex-state')).toHaveText('collecting');
    await expect(page.locator('#connector-codex-last-turn')).toHaveText('no turn stored yet');
    await screenshotBothThemes(page, 'status-connectors-collecting');
  });
});

test('a connector with an error shows failed with its reason, in the sidebar too', async ({
  session,
}) => {
  const reason = 'the session files cannot be listed';
  const reply = runningReply({
    connectors: {
      'claude-code': { turns: 2, skipped: 0, scanning: false, error: reason },
      codex: { turns: 4, skipped: 0, scanning: false, last_turn: secondsAgo(5) },
    },
  });
  await withStandIn(session, reply, async (page) => {
    await page.getByRole('button', { name: 'Status', exact: true }).click();
    await expect(page.locator('#sidebar-collection')).toHaveText('collection failed');
    await expect(page.locator('#connector-claude-code-state')).toHaveText('failed');
    await expect(page.locator('#connector-claude-code-state')).toHaveAttribute(
      'data-state',
      'failed',
    );
    await expect(page.locator('#connector-claude-code-reason')).toHaveText(reason);
    await expect(page.locator('#connector-codex-state')).toHaveText('collecting');
    await screenshotBothThemes(page, 'status-connectors-failed');
  });
});

const quietStates = [
  { name: 'off', sidebar: 'collection off', extra: { off: true } },
  { name: 'scanning', sidebar: 'scanning', extra: { scanning: true } },
] as const;

for (const { name, sidebar, extra } of quietStates) {
  test(`collection that is ${name} shows ${name} in the sidebar and on the page`, async ({
    session,
  }) => {
    const reply = runningReply({
      connectors: {
        'claude-code': { turns: 3, skipped: 0, scanning: false, ...extra },
        codex: { turns: 0, skipped: 0, scanning: false, ...extra },
      },
    });
    await withStandIn(session, reply, async (page) => {
      await page.getByRole('button', { name: 'Status', exact: true }).click();
      await expect(page.locator('#sidebar-collection')).toHaveText(sidebar);
      await expect(page.locator('#connector-claude-code-state')).toHaveText(name);
      await expect(page.locator('#connector-codex-state')).toHaveText(name);
      await screenshotBothThemes(page, `status-connectors-${name}`);
    });
  });
}

test('the last turn reads in minutes, then as a date, and a tool the app has no name for shows as sent', async ({
  session,
}) => {
  const reply = runningReply({
    connectors: {
      'claude-code': { turns: 5, skipped: 0, scanning: false, last_turn: secondsAgo(300) },
      codex: { turns: 6, skipped: 0, scanning: false, last_turn: secondsAgo(3 * 3600) },
      'another-tool': { turns: 1, skipped: 0, scanning: false },
    },
  });
  await withStandIn(session, reply, async (page) => {
    await page.getByRole('button', { name: 'Status', exact: true }).click();
    await expect(page.locator('#connector-claude-code-last-turn')).toHaveText(
      /^last turn [45] min ago$/,
    );
    await expect(page.locator('#connector-codex-last-turn')).toHaveText(
      /^last turn \d{1,2} [A-Z][a-z]{2} \d{4}, \d{2}:\d{2}$/,
    );
    await expect(page.locator('#connector-another-tool').getByRole('heading')).toHaveText(
      'another-tool',
    );
    // Nothing was stored within the last minute, so the sidebar says nothing about collection.
    await expect(page.locator('#sidebar-collection')).toHaveCount(0);
  });
});

test('an agent that sends no connectors shows no collection line and no connector cards', async ({
  session,
}) => {
  await withStandIn(session, runningReply({}), async (page) => {
    await page.getByRole('button', { name: 'Status', exact: true }).click();
    await expect(page.locator('#agent-status')).toHaveText('running');
    await expect(page.locator('#sidebar-collection')).toHaveCount(0);
    await expect(page.locator('[id^="connector-"]')).toHaveCount(0);
  });
});
