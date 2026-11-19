import type { Page } from '@playwright/test';
import { expect, openSettings, test, withStandIn } from './fixtures';
import type { Session } from './fixtures';
import { screenshotBothThemes } from './screenshots';

/** The section at the top of the window, so a screenshot shows the section and not the form above it. */
async function shoot(page: Page, name: string): Promise<void> {
  await page.locator('#setup-section-title').evaluate((heading) => {
    heading.scrollIntoView({ block: 'start' });
  });
  await screenshotBothThemes(page, name);
}

type Phase = 'idle' | 'writing_files' | 'reading_and_building' | 'done' | 'failed' | 'cancelled';

/** What the stand-in agent answers, and what it has been asked. */
interface Agent {
  phase: Phase;
  modelSet: boolean;
  sessions: number;
  textBytes: number;
  sent: number;
  received: number;
  report: string;
  /** The reason the next start fails with: the run starts and ends `failed`. */
  failNext: string | null;
  /** The agent answers every setup command with an error, as one that cannot be reached would. */
  down: boolean;
  readonly lines: string[];
}

function newAgent(overrides: Partial<Agent> = {}): Agent {
  return {
    phase: 'idle',
    modelSet: true,
    sessions: 0,
    textBytes: 0,
    sent: 0,
    received: 0,
    report: '',
    failNext: null,
    down: false,
    lines: [],
    ...overrides,
  };
}

const report = [
  '## Trackers',
  '',
  '- Tracker one: 3 projects',
  '- Tracker two: 2 projects',
  '',
  'Read [how the dashboard works](https://example.invalid/guide).',
].join('\n');

function statusReply(agent: Agent, error?: string): string {
  return JSON.stringify({
    state: agent.phase,
    sessions: agent.sessions,
    text_bytes: agent.textBytes,
    sent: agent.sent,
    received: agent.received,
    ...(agent.phase === 'failed' ? { error } : {}),
    ...(agent.phase === 'done' ? { report: agent.report } : {}),
  });
}

function settingsReply(agent: Agent): string {
  return JSON.stringify({
    model: agent.modelSet
      ? { base_url: 'https://example.invalid/v1', model: 'example-model', api_key_set: true }
      : { base_url: '', model: '', api_key_set: false },
    collection: { enabled: true, claude_code: true, codex: true, screen: false },
    supports: { screen: false },
    notice: null,
  });
}

function reply(agent: Agent, line: string): string {
  agent.lines.push(line.split(' ')[0] ?? '');
  if (agent.down && line.startsWith('dashboard-setup-')) {
    return JSON.stringify({ error: 'the agent is not available' });
  }
  if (line === 'dashboard-setup-start {"days":30}') {
    if (agent.failNext !== null) {
      const reason = agent.failNext;
      agent.failNext = null;
      agent.phase = 'failed';
      return statusReply(agent, reason);
    }
    agent.phase = 'writing_files';
    agent.sessions = 0;
    agent.textBytes = 0;
    agent.sent = 0;
    agent.received = 0;
    return statusReply(agent);
  }
  if (line === 'dashboard-setup-status') {
    return statusReply(agent, 'there is no session to read in this period');
  }
  if (line === 'dashboard-setup-cancel') {
    agent.phase = 'cancelled';
    return statusReply(agent);
  }
  if (line.startsWith('dashboard-numbers ')) {
    return JSON.stringify({
      total_minutes: 600,
      session_count: 12,
      hidden_project_count: 0,
      groups: [],
      days: [{ label: '2026-11-18', minutes: 600 }],
    });
  }
  if (line === 'settings-get') {
    return settingsReply(agent);
  }
  return JSON.stringify({ status: 'running', version: '0.1.0', pid: 42, model: 'example' });
}

/** Answers the window's calls with `agent` and opens the Settings page. */
async function withAgent(
  session: Session,
  agent: Agent,
  check: (page: Page) => Promise<void>,
): Promise<void> {
  await withStandIn(
    session,
    (line) => reply(agent, line),
    async (page) => {
      await openSettings(page, 'Dashboard');
      await check(page);
    },
  );
}

const start = (page: Page) => page.locator('#setup-start');
const steps = (page: Page) => page.locator('#setup-steps');
const sizes = (page: Page) => page.locator('#setup-sizes');

/** A run that is at the first step with some files written. */
function writing(agent: Agent): void {
  agent.phase = 'writing_files';
  agent.sessions = 2;
  agent.textBytes = 1536;
}

function finish(agent: Agent): void {
  agent.phase = 'done';
  agent.sessions = 12;
  agent.textBytes = 48 * 1024;
  agent.sent = 3 * 1024 * 1024;
  agent.received = 20 * 1024;
  agent.report = report;
}

test('a press shows the steps and the sizes, then the report and a link to the dashboard', async ({
  session,
}) => {
  const agent = newAgent();
  await withAgent(session, agent, async (page) => {
    await expect(start(page)).toHaveText('Enable my dashboard');
    await expect(start(page)).toBeEnabled();
    await expect(page.locator('#setup-hint')).toHaveText(
      'The last 30 days hold 12 sessions with activity.',
    );
    await shoot(page, 'setup-button');

    await start(page).click();
    await expect(steps(page)).toBeVisible();
    await expect(page.locator('[data-step="writing_files"]')).toHaveAttribute(
      'data-state',
      'running',
    );
    await expect(page.locator('[data-step="reading_and_building"]')).toHaveAttribute(
      'data-state',
      'waiting',
    );
    expect(agent.lines).toContain('dashboard-setup-start');

    // The page asks again each second: the sizes follow the agent.
    writing(agent);
    await expect(sizes(page)).toContainText('Read so far: 2 sessions, 1.5 KB of text.');
    await expect(page.locator('#setup-cancel')).toBeVisible();

    agent.phase = 'reading_and_building';
    agent.sessions = 12;
    agent.textBytes = 48 * 1024;
    agent.sent = 2048;
    agent.received = 512;
    await expect(page.locator('[data-step="writing_files"]')).toHaveAttribute('data-state', 'done');
    await expect(page.locator('[data-step="reading_and_building"]')).toHaveAttribute(
      'data-state',
      'running',
    );
    await expect(sizes(page)).toContainText('Model: 2.0 KB sent, 512 B received.');
    await shoot(page, 'setup-loader');

    finish(agent);
    await expect(page.locator('#setup-done')).toHaveText('The dashboard is built.');
    await expect(page.getByRole('heading', { name: 'What was found' })).toBeVisible();
    await expect(page.locator('#setup-report')).toContainText('Tracker one: 3 projects');
    await expect(page.locator('#setup-report').getByText('how the dashboard works')).toBeVisible();
    await expect(page.locator('#setup-steps')).toHaveCount(0);
    await expect(start(page)).toHaveText('Rebuild my dashboard');
    await shoot(page, 'setup-report');

    // The link goes to the Dashboard page.
    await page.locator('#setup-open-dashboard').click();
    await expect(page.getByRole('heading', { name: 'Dashboard', level: 1 })).toBeVisible();
  });
});

test('Cancel ends the run and the button is there again', async ({ session }) => {
  const agent = newAgent();
  writing(agent);
  await withAgent(session, agent, async (page) => {
    // A window opened in the middle of a run shows the run.
    await expect(steps(page)).toBeVisible();
    await expect(sizes(page)).toContainText('Read so far: 2 sessions');
    await page.locator('#setup-cancel').click();
    await expect(page.locator('#setup-cancelled')).toHaveText(
      'The setup was cancelled. The dashboard is as it was before.',
    );
    expect(agent.lines).toContain('dashboard-setup-cancel');
    await expect(steps(page)).toHaveCount(0);
    await expect(start(page)).toHaveText('Enable my dashboard');
    await expect(start(page)).toBeEnabled();
  });
});

test('a failure shows its reason, and Retry starts the run again', async ({ session }) => {
  const agent = newAgent({ failNext: 'there is no session to read in this period' });
  await withAgent(session, agent, async (page) => {
    await start(page).click();
    await expect(page.locator('#setup-failed')).toHaveText(
      'The setup failed: there is no session to read in this period',
    );
    await expect(start(page)).toHaveText('Retry');
    await shoot(page, 'setup-failed');

    await start(page).click();
    await expect(steps(page)).toBeVisible();
    await expect(page.locator('#setup-failed')).toHaveCount(0);
    expect(agent.lines.filter((line) => line === 'dashboard-setup-start')).toHaveLength(2);
  });
});

test('Rebuild my dashboard starts a second run', async ({ session }) => {
  const agent = newAgent();
  finish(agent);
  await withAgent(session, agent, async (page) => {
    await expect(page.locator('#setup-report')).toContainText('Tracker two: 2 projects');
    await expect(start(page)).toHaveText('Rebuild my dashboard');
    // The hint holds the sessions of the period and the text the last run read.
    await expect(page.locator('#setup-hint')).toHaveText(
      'The last 30 days hold 12 sessions with activity. The last run read 48.0 KB of text.',
    );
    await start(page).click();
    await expect(steps(page)).toBeVisible();
    await expect(page.locator('#setup-report')).toHaveCount(0);
    expect(agent.lines.filter((line) => line === 'dashboard-setup-start')).toHaveLength(1);

    finish(agent);
    await expect(page.locator('#setup-done')).toBeVisible();
    await expect(start(page)).toHaveText('Rebuild my dashboard');
  });
});

test('with no model the button explains why and the link goes to the model section', async ({
  session,
}) => {
  const agent = newAgent({ modelSet: false });
  await withAgent(session, agent, async (page) => {
    await expect(start(page)).toBeDisabled();
    await expect(page.locator('#setup-no-model')).toContainText('No model is set');
    await expect(start(page)).toHaveAttribute('aria-describedby', 'setup-no-model');
    await shoot(page, 'setup-no-model');

    await page.locator('#setup-go-to-model').click();
    await expect(page.locator('#model-base-url')).toBeFocused();
    expect(agent.lines).not.toContain('dashboard-setup-start');
  });
});

test('an agent that cannot be reached mid-run is said, and the run is shown again when it answers', async ({
  session,
}) => {
  const agent = newAgent();
  writing(agent);
  await withAgent(session, agent, async (page) => {
    await expect(steps(page)).toBeVisible();
    await expect(page.locator('#setup-unreachable')).toHaveCount(0);

    agent.down = true;
    await expect(page.locator('#setup-unreachable')).toContainText('the agent is not available');
    // The run is still there, and so is Cancel; a press that fails says so too.
    await expect(steps(page)).toBeVisible();
    await expect(page.locator('#setup-cancel')).toBeEnabled();
    await page.locator('#setup-cancel').click();
    await expect(page.locator('#setup-unreachable')).toBeVisible();
    await expect(page.locator('#setup-cancel')).toBeEnabled();
    await shoot(page, 'setup-unreachable');

    agent.down = false;
    await expect(page.locator('#setup-unreachable')).toHaveCount(0);
    await expect(sizes(page)).toContainText('Read so far: 2 sessions');
  });
});
