import type { Page } from '@playwright/test';
import type { Whose } from '../src/dashboard-protocol';
import { expect, test, withStandIn } from './fixtures';
import type { Session } from './fixtures';
import { screenshotBothThemes } from './screenshots';

interface Request {
  readonly days: { readonly label: string; readonly start: string; readonly end: string }[];
  readonly sessions: Whose;
}

/** What the stand-in agent answers, and what it has been asked. */
interface Agent {
  collecting: boolean;
  /** Minutes the stand-in finds in a period: none, or a fixed pattern. */
  empty: boolean;
  /** The agent's error text for the numbers, when it cannot give them. */
  numbersError: string | null;
  /** The agent's error text for the settings, when it cannot give them. */
  settingsError: string | null;
  /** While set, the numbers wait for it: the page is reading. */
  hold: Promise<void> | null;
  readonly requests: Request[];
}

function newAgent(overrides: Partial<Agent> = {}): Agent {
  return {
    collecting: true,
    empty: false,
    numbersError: null,
    settingsError: null,
    hold: null,
    requests: [],
    ...overrides,
  };
}

/** The user's own time: Monday 10 min, Tuesday 20 min ... Sunday 70 min, 4 h 40 min in a week. */
const ownMinutes = (index: number): number => (index + 1) * 10;

/** The unattended agents worked 30 minutes, on the first day of the period only. */
const unattendedMinutes = (index: number): number => (index === 0 ? 30 : 0);

function minutesOf(whose: Whose, index: number): number {
  const own = whose === 'unattended' ? 0 : ownMinutes(index);
  const unattended = whose === 'own' ? 0 : unattendedMinutes(index);
  return own + unattended;
}

function numbersReply(request: Request, agent: Agent): string {
  if (agent.numbersError !== null) {
    return JSON.stringify({ error: agent.numbersError });
  }
  const days = request.days.map((day, index) => ({
    label: day.label,
    minutes: agent.empty ? 0 : minutesOf(request.sessions, index),
  }));
  return JSON.stringify({
    total_minutes: days.reduce((sum, day) => sum + day.minutes, 0),
    session_count: days.filter((day) => day.minutes > 0).length,
    hidden_project_count: 0,
    groups: [],
    days,
  });
}

function settingsReply(agent: Agent): string {
  if (agent.settingsError !== null) {
    return JSON.stringify({ error: agent.settingsError });
  }
  const on = agent.collecting;
  return JSON.stringify({
    model: { base_url: 'https://example.invalid/v1', model: 'example-model', api_key_set: false },
    collection: { enabled: on, claude_code: on, codex: on },
    notice: null,
  });
}

/** The window's clock stands at Wednesday 18 November 2026, so the week is a known one. */
const now = new Date(2026, 10, 18, 10, 0, 0);

/** Answers the window's calls with `agent` and opens the Dashboard page. */
async function withAgent(
  session: Session,
  agent: Agent,
  check: (page: Page) => Promise<void>,
): Promise<void> {
  await withStandIn(
    session,
    async (line) => {
      if (line.startsWith('dashboard-numbers ')) {
        const request = JSON.parse(line.slice('dashboard-numbers '.length)) as Request;
        agent.requests.push(request);
        await (agent.hold ?? Promise.resolve());
        return numbersReply(request, agent);
      }
      if (line === 'settings-get') {
        return settingsReply(agent);
      }
      return JSON.stringify({ status: 'running', version: '0.1.0', pid: 42, model: 'example' });
    },
    async (page) => {
      await page.clock.install({ time: now });
      await page.reload();
      await page.getByRole('button', { name: 'Dashboard', exact: true }).click();
      await check(page);
    },
  );
}

const period = (page: Page) => page.locator('#dashboard-period');
const weekTotal = (page: Page) => page.locator('#dashboard-week-total');
const dayTotal = (page: Page) => page.locator('#dashboard-day-total');

test('the Dashboard opens on this week, with the user’s own time', async ({ session }) => {
  const agent = newAgent();
  await withAgent(session, agent, async (page) => {
    await expect(period(page)).toHaveText('Week of Nov 16 to Nov 22');
    await expect(weekTotal(page)).toHaveText('4 h 40 min');
    // Wednesday is the third day of the stand-in's week.
    await expect(dayTotal(page)).toHaveText('30 min');
    await expect(page.getByText('Day Nov 18', { exact: true })).toBeVisible();
    await expect(page.locator('#dashboard-sessions')).toHaveText('7');
    await expect(page.locator('#dashboard-whose [data-pressed]')).toHaveText('Mine');
    // The window sends the seven days of the week, Monday first, and asks for the own time.
    expect(agent.requests).toHaveLength(1);
    expect(agent.requests[0]?.sessions).toBe('own');
    expect(agent.requests[0]?.days.map((day) => day.label)).toEqual([
      '2026-11-16',
      '2026-11-17',
      '2026-11-18',
      '2026-11-19',
      '2026-11-20',
      '2026-11-21',
      '2026-11-22',
    ]);
    await screenshotBothThemes(page, 'dashboard-numbers');
  });
});

test('the arrows and the date picker change the period', async ({ session }) => {
  const agent = newAgent();
  await withAgent(session, agent, async (page) => {
    await expect(period(page)).toHaveText('Week of Nov 16 to Nov 22');

    // One day on: the same week, and the Day card shows Thursday.
    await page.getByRole('button', { name: 'Next day', exact: true }).click();
    await expect(page.getByText('Day Nov 19', { exact: true })).toBeVisible();
    await expect(dayTotal(page)).toHaveText('40 min');
    await expect(period(page)).toHaveText('Week of Nov 16 to Nov 22');
    // Days of one week come in one answer: nothing is read again.
    expect(agent.requests).toHaveLength(1);

    // Four days on, from Thursday, reaches Monday of the next week.
    for (let step = 0; step < 4; step += 1) {
      await page.getByRole('button', { name: 'Next day', exact: true }).click();
    }
    await expect(period(page)).toHaveText('Week of Nov 23 to Nov 29');
    await expect(page.getByText('Day Nov 23', { exact: true })).toBeVisible();
    await expect(dayTotal(page)).toHaveText('10 min');

    await page.getByRole('button', { name: 'Previous day', exact: true }).click();
    await expect(period(page)).toHaveText('Week of Nov 16 to Nov 22');
    await expect(page.getByText('Day Nov 22', { exact: true })).toBeVisible();

    await page.getByRole('button', { name: 'Next week', exact: true }).click();
    await expect(period(page)).toHaveText('Week of Nov 23 to Nov 29');
    await expect(page.getByText('Day Nov 29', { exact: true })).toBeVisible();
    await page.getByRole('button', { name: 'Previous week', exact: true }).click();
    await expect(period(page)).toHaveText('Week of Nov 16 to Nov 22');

    // Any date: Tuesday 10 November, the week before last.
    await page.locator('#dashboard-date').click();
    await page.getByRole('button', { name: /November 10th,/ }).click();
    await expect(period(page)).toHaveText('Week of Nov 9 to Nov 15');
    await expect(page.getByText('Day Nov 10', { exact: true })).toBeVisible();
    await expect(dayTotal(page)).toHaveText('20 min');
    expect(agent.requests.at(-1)?.days.map((day) => day.label)).toContain('2026-11-10');
  });
});

test('the switch changes whose time the totals show', async ({ session }) => {
  const agent = newAgent();
  await withAgent(session, agent, async (page) => {
    await expect(weekTotal(page)).toHaveText('4 h 40 min');

    await page.getByRole('button', { name: 'Unattended', exact: true }).click();
    await expect(weekTotal(page)).toHaveText('30 min');
    expect(agent.requests.at(-1)?.sessions).toBe('unattended');

    await page.getByRole('button', { name: 'Both', exact: true }).click();
    await expect(weekTotal(page)).toHaveText('5 h 10 min');
    expect(agent.requests.at(-1)?.sessions).toBe('all');

    // Pressing the chosen item again keeps the choice.
    await page.getByRole('button', { name: 'Both', exact: true }).click();
    await expect(page.locator('#dashboard-whose [data-pressed]')).toHaveText('Both');

    await page.getByRole('button', { name: 'Mine', exact: true }).click();
    await expect(weekTotal(page)).toHaveText('4 h 40 min');
  });
});

test('the page says when it is reading the history', async ({ session }) => {
  let release: () => void = () => undefined;
  const agent = newAgent({
    hold: new Promise<void>((resolve) => {
      release = resolve;
    }),
  });
  await withAgent(session, agent, async (page) => {
    await expect(page.getByText('Reading the history')).toBeVisible();
    await expect(weekTotal(page)).toHaveCount(0);
    await screenshotBothThemes(page, 'dashboard-reading');
    release();
    await expect(weekTotal(page)).toHaveText('4 h 40 min');
    await expect(page.getByText('Reading the history')).toHaveCount(0);
  });
});

test('the page says when there is nothing to show', async ({ session }) => {
  await withAgent(session, newAgent({ empty: true }), async (page) => {
    await expect(page.locator('#dashboard-empty')).toContainText('Nothing to show');
    await expect(weekTotal(page)).toHaveCount(0);
    await screenshotBothThemes(page, 'dashboard-empty');
  });
});

test('the page says when collection is off, and links to Settings', async ({ session }) => {
  await withAgent(session, newAgent({ empty: true, collecting: false }), async (page) => {
    await expect(page.locator('#dashboard-off')).toContainText('Collection is off');
    await screenshotBothThemes(page, 'dashboard-collection-off');
    await page.locator('#dashboard-open-settings').click();
    await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible();
  });
});

test('with collection off, the numbers of the past still show, with a note and the link', async ({
  session,
}) => {
  await withAgent(session, newAgent({ collecting: false }), async (page) => {
    await expect(weekTotal(page)).toHaveText('4 h 40 min');
    await expect(page.locator('#dashboard-off-note')).toContainText('Collection is off');
    await expect(page.locator('#dashboard-off')).toHaveCount(0);
    await page.locator('#dashboard-open-settings').click();
    await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible();
  });
});

test('the page shows the error of the agent, and not numbers, when the agent cannot give them', async ({
  session,
}) => {
  const error = 'the activity time could not be read';
  await withAgent(session, newAgent({ numbersError: error }), async (page) => {
    await expect(page.locator('#dashboard-failed')).toHaveText(
      `The dashboard is not available: ${error}`,
    );
    await expect(weekTotal(page)).toHaveCount(0);
  });
});

test('the page does not call collection on when the settings cannot be read', async ({
  session,
}) => {
  const error = 'the settings could not be read';
  await withAgent(session, newAgent({ empty: true, settingsError: error }), async (page) => {
    await expect(page.locator('#dashboard-failed')).toHaveText(
      `The dashboard is not available: ${error}`,
    );
    await expect(page.locator('#dashboard-empty')).toHaveCount(0);
  });
});
