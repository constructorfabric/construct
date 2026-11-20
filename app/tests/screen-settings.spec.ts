import type { ElectronApplication, Page } from '@playwright/test';
import { settingsReply as sharedSettingsReply } from './dashboard-shared';
import { expect, openSettings, recordOpenedLinks, test, withStandIn } from './fixtures';
import type { Session } from './fixtures';
import { screenshotBothThemes } from './screenshots';

// The page asks for the status every 3 seconds.
const pollDeadlineMs = 10_000;

type GrantState = 'granted' | 'denied' | 'not_asked' | 'browser_not_running';

interface Agent {
  supported: boolean;
  enabled: boolean;
  screen: boolean;
  grants: { name: string; state: GrantState }[];
  readonly lines: string[];
}

function newAgent(overrides: Partial<Agent> = {}): Agent {
  return {
    supported: true,
    enabled: true,
    screen: false,
    grants: [
      { name: 'accessibility', state: 'granted' },
      { name: 'screen_recording', state: 'denied' },
      { name: 'automation', state: 'browser_not_running' },
      { name: 'input_monitoring', state: 'granted' },
    ],
    lines: [],
    ...overrides,
  };
}

function settingsReply(agent: Agent): string {
  return sharedSettingsReply(agent.enabled, { supported: agent.supported, on: agent.screen });
}

function reply(agent: Agent, line: string): string {
  agent.lines.push(line);
  if (line === 'settings-get') {
    return settingsReply(agent);
  }
  if (line.startsWith('settings-set ')) {
    const change = JSON.parse(line.slice('settings-set '.length)) as {
      collection?: { enabled?: boolean; screen?: boolean };
    };
    agent.enabled = change.collection?.enabled ?? agent.enabled;
    agent.screen = change.collection?.screen ?? agent.screen;
    return settingsReply(agent);
  }
  if (line === 'dashboard-setup-status') {
    return JSON.stringify({ state: 'idle', sessions: 0, text_bytes: 0, sent: 0, received: 0 });
  }
  return JSON.stringify({
    status: 'running',
    version: '0.1.0',
    pid: 42,
    model: 'example-model',
    screen: {
      state: agent.screen ? 'needs_permission' : 'off',
      samples_today: 0,
      grants: agent.grants,
    },
  });
}

/** Answers the window's calls with `agent` and opens the Settings page. */
async function withAgent(
  session: Session,
  agent: Agent,
  check: (page: Page, app: ElectronApplication) => Promise<void>,
): Promise<void> {
  await withStandIn(
    session,
    (line) => reply(agent, line),
    async (page, app) => {
      await openSettings(page, 'Collection');
      await check(page, app);
    },
  );
}

/** Each grant the page knows: its name, its sentence and the pane its button opens. */
const rows = [
  {
    name: 'accessibility',
    title: 'Accessibility',
    sentence: 'Lets the app read the title of the window in front.',
    pane: 'Privacy_Accessibility',
  },
  {
    name: 'screen_recording',
    title: 'Screen recording',
    sentence: 'Lets the app read the titles of the windows of other apps.',
    pane: 'Privacy_ScreenCapture',
  },
  {
    name: 'automation',
    title: 'Automation',
    sentence: 'Lets the app ask your browser for the title and address of the open tab.',
    pane: 'Privacy_Automation',
  },
  {
    name: 'input_monitoring',
    title: 'Input monitoring',
    sentence: 'Lets the app tell when the keyboard or the mouse was last used.',
    pane: 'Privacy_ListenEvent',
  },
] as const;

const screen = (page: Page) => page.getByRole('switch', { name: 'Apps and websites' });
const grantState = (page: Page, name: string) => page.locator(`#screen-grant-${name}-state`);

test('a computer that cannot collect shows no switch and no permissions', async ({ page }) => {
  // The agent of this repository has no screen signals yet, so it says the computer is not supported.
  await openSettings(page, 'Collection');
  await expect(page.getByRole('switch', { name: 'Claude Code' })).toBeVisible();
  await expect(screen(page)).toHaveCount(0);
  await expect(page.locator('#screen-grants')).toHaveCount(0);
  await expect(page.getByText('Apps and websites')).toHaveCount(0);
});

test('a stand-in agent that does not support it shows nothing of it either', async ({
  session,
}) => {
  await withAgent(session, newAgent({ supported: false, screen: true }), async (page) => {
    await expect(page.getByRole('switch', { name: 'Codex' })).toBeVisible();
    await expect(screen(page)).toHaveCount(0);
    await expect(page.locator('#screen-grants')).toHaveCount(0);
  });
});

test('the switch is off by default and shows no permissions until it is turned on', async ({
  session,
}) => {
  const agent = newAgent();
  await withAgent(session, agent, async (page) => {
    await expect(screen(page)).toBeVisible();
    await expect(screen(page)).not.toBeChecked();
    await expect(page.locator('#screen-grants')).toHaveCount(0);

    await screen(page).click();
    await expect(screen(page)).toBeChecked();
    expect(agent.lines).toContain('settings-set {"collection":{"screen":true}}');
    await expect(page.locator('#screen-grants')).toBeVisible();

    await screen(page).click();
    await expect(screen(page)).not.toBeChecked();
    await expect(page.locator('#screen-grants')).toHaveCount(0);
  });
});

test('the switch follows the master switch: off under it, and kept for when it is on again', async ({
  session,
}) => {
  await withAgent(session, newAgent({ screen: true }), async (page) => {
    await expect(screen(page)).toBeChecked();
    await page.getByRole('switch', { name: 'Collect from my coding tools' }).click();
    await expect(screen(page)).not.toBeChecked();
    await expect(screen(page)).toBeDisabled();
    await expect(page.locator('#screen-grants')).toHaveCount(0);

    await page.getByRole('switch', { name: 'Collect from my coding tools' }).click();
    await expect(screen(page)).toBeChecked();
    await expect(page.locator('#screen-grants')).toBeVisible();
  });
});

test('each grant shows its state in words, one sentence on what it reads and a button', async ({
  session,
}) => {
  await withAgent(session, newAgent({ screen: true }), async (page) => {
    await expect(grantState(page, 'accessibility')).toHaveText('Allowed');
    await expect(grantState(page, 'screen_recording')).toHaveText('Not allowed');
    await expect(grantState(page, 'automation')).toHaveText('Browser not running');
    for (const row of rows) {
      await expect(page.locator(`#screen-grant-${row.name}`)).toContainText(row.sentence);
      await expect(page.locator(`#screen-grant-${row.name}-open`)).toHaveText(
        'Open System Settings',
      );
      await expect(
        page.getByRole('button', { name: `Open System Settings: ${row.title}` }),
      ).toBeVisible();
    }
    // The section at the top of the window, so the screenshot shows the section and not the form above it.
    await page.locator('#collection-section-title').evaluate((heading) => {
      heading.scrollIntoView({ block: 'start' });
    });
    await screenshotBothThemes(page, 'screen-settings-grants');
  });
});

test('a grant that was not asked yet reads as not allowed', async ({ session }) => {
  const grants = [{ name: 'accessibility', state: 'not_asked' as const }];
  await withAgent(session, newAgent({ screen: true, grants }), async (page) => {
    await expect(grantState(page, 'accessibility')).toHaveText('Not allowed');
    await expect(grantState(page, 'accessibility')).toHaveAttribute('data-state', 'not_asked');
  });
});

test('the state changes by itself when the person gives or takes a grant', async ({ session }) => {
  const agent = newAgent({ screen: true });
  await withAgent(session, agent, async (page) => {
    await expect(grantState(page, 'screen_recording')).toHaveText('Not allowed');
    await expect(grantState(page, 'accessibility')).toHaveText('Allowed');

    agent.grants = [
      { name: 'accessibility', state: 'denied' },
      { name: 'screen_recording', state: 'granted' },
      { name: 'automation', state: 'granted' },
    ];
    await expect(grantState(page, 'screen_recording')).toHaveText('Allowed', {
      timeout: pollDeadlineMs,
    });
    await expect(grantState(page, 'accessibility')).toHaveText('Not allowed');
    await expect(grantState(page, 'automation')).toHaveText('Allowed');
  });
});

test('a grant the app does not know is not shown', async ({ session }) => {
  const grants = [
    { name: 'accessibility', state: 'granted' as const },
    { name: 'something_new', state: 'granted' as const },
  ];
  await withAgent(session, newAgent({ screen: true, grants }), async (page) => {
    await expect(page.locator('#screen-grants li')).toHaveCount(1);
  });
});

test('the button of each grant asks the main process to open its pane, by name', async ({
  session,
}) => {
  await withAgent(session, newAgent({ screen: true }), async (page, app) => {
    const opened = await recordOpenedLinks(app);
    await expect(grantState(page, 'screen_recording')).toBeVisible();

    for (const [index, row] of rows.entries()) {
      await page.locator(`#screen-grant-${row.name}-open`).click();
      await expect.poll(opened).toHaveLength(index + 1);
      expect((await opened())[index]).toBe(
        `x-apple.systempreferences:com.apple.preference.security?${row.pane}`,
      );
    }
  });
});

test('the main process opens nothing for an address or a name that is not on its list', async ({
  session,
}) => {
  await withAgent(session, newAgent({ screen: true }), async (page, app) => {
    const opened = await recordOpenedLinks(app);
    const answers = await page.evaluate(() =>
      Promise.all(
        [
          'https://example.com',
          'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility',
          'nothing',
        ].map((pane) => window.constructAgent.openPane(pane)),
      ),
    );
    expect(answers).toEqual([false, false, false]);
    expect(await opened()).toEqual([]);
  });
});

test('a pane that cannot be opened is said, not thrown', async ({ session }) => {
  await withAgent(session, newAgent({ screen: true }), async (page, app) => {
    await app.evaluate(({ shell }) => {
      shell.openExternal = () => Promise.reject(new Error('refused'));
    });
    await page.locator('#screen-grant-accessibility-open').click();
    await expect(page.locator('#screen-grants-open-failed')).toHaveText(
      'System Settings could not be opened.',
    );
  });
});

test('an empty list of grants says the state is not known', async ({ session }) => {
  await withAgent(session, newAgent({ screen: true, grants: [] }), async (page) => {
    await expect(page.locator('#screen-grants-unknown')).toHaveText(
      'The state of the permissions is not known right now.',
    );
    await expect(page.locator('#screen-grants')).toHaveCount(0);
  });
});

test('a pane call that is rejected says so instead of leaving an unhandled error', async ({
  session,
}) => {
  await withAgent(session, newAgent({ screen: true }), async (page, app) => {
    // The main process answers the call with an error, so the invoke rejects in the window.
    await app.evaluate(({ ipcMain }) => {
      ipcMain.removeHandler('external:open-pane');
      ipcMain.handle('external:open-pane', () => {
        throw new Error('the call failed');
      });
    });
    const unhandled: string[] = [];
    page.on('pageerror', (error) => unhandled.push(error.message));
    await page.locator('#screen-grant-accessibility-open').click();
    await expect(page.locator('#screen-grants-open-failed')).toBeVisible();
    expect(unhandled).toEqual([]);
  });
});
