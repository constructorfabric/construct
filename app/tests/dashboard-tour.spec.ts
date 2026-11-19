import { mkdirSync, readdirSync, renameSync, rmSync } from 'node:fs';
import path from 'node:path';
import { appDir, expect, test, withStandIn } from './fixtures';
import { now, settingsReply } from './dashboard-shared';
import { screenshotBothThemes } from './screenshots';

// A walk through the Dashboard against a stand-in agent with made-up numbers. It saves a screenshot
// of each step, in both themes, and a recording of the whole walk to `test-results/`, to show the
// page to someone who does not run the app. Nothing here is read from a real machine.

const resultsDir = path.join(appDir, 'test-results');
const videoDir = path.join(resultsDir, 'dashboard-tour-video');

/** The size of the window, and of the recording: small enough for a video of a minute to stay light. */
const size = { width: 1280, height: 800 };

/** How long a step stays on the screen, so that a person watching the recording can read it. */
const linger = 3_300;

interface Agent {
  collecting: boolean;
  empty: boolean;
  /** While set, the numbers wait for it: the page is reading the history. */
  hold: Promise<void> | null;
}

interface Day {
  readonly label: string;
  readonly start: string;
  readonly end: string;
}

interface Request {
  readonly days: readonly Day[];
  readonly widget?: string;
  readonly group?: string;
  readonly offset?: number;
  readonly limit?: number;
}

const document = {
  type: 'a dashboard type',
  title: 'Activity',
  payload: {
    period: 'this_week',
    sessions: 'own',
    widgets: [
      { id: 'total', kind: 'total', position: { column: 0, row: 0, width: 12, height: 2 } },
      {
        id: 'per-day',
        kind: 'bars_per_day',
        position: { column: 0, row: 2, width: 12, height: 4 },
      },
      {
        id: 'by-project',
        kind: 'table',
        group_by: 'project',
        position: { column: 0, row: 6, width: 6, height: 5 },
      },
      {
        id: 'by-ticket',
        kind: 'table',
        group_by: 'ticket',
        position: { column: 6, row: 6, width: 6, height: 5 },
      },
      {
        id: 'end-of-day',
        kind: 'day_summary',
        position: { column: 0, row: 11, width: 12, height: 3 },
      },
    ],
    hidden_projects: [],
    project_names: {},
  },
};

/** Shares of a week's time for each group of a table, as made-up as the rest. */
const shares: Record<string, readonly (readonly [string, number])[]> = {
  'by-project': [
    ['billing', 0.45],
    ['search', 0.35],
    ['docs', 0.2],
  ],
  'by-ticket': [
    ['alpha-2', 0.4],
    ['alpha-1', 0.35],
    ['No ticket', 0.25],
  ],
};

/** Minutes of a day: a working day has between 1 and 5 hours, a weekend day has little. */
function minutesOf(label: string): number {
  const date = new Date(`${label}T12:00:00Z`);
  const weekend = date.getUTCDay() === 0 || date.getUTCDay() === 6;
  const pseudoRandom = (date.getUTCDate() * 37 + date.getUTCMonth() * 11) % 240;
  return weekend ? pseudoRandom % 25 : 60 + pseudoRandom;
}

function numbersReply(request: Request, empty: boolean): string {
  const days = request.days.map((day) => ({
    label: day.label,
    minutes: empty ? 0 : minutesOf(day.label),
  }));
  const total = days.reduce((sum, day) => sum + day.minutes, 0);
  const groups = empty ? [] : (shares[request.widget ?? ''] ?? []);
  return JSON.stringify({
    total_minutes: total,
    session_count: days.filter((day) => day.minutes > 0).length,
    hidden_project_count: 0,
    groups: groups.map(([key, share]) => ({
      key,
      minutes: Math.round(total * share),
      session_count: Math.round(share * 10),
      share,
    })),
    days,
  });
}

const sessionCount = 7;

function sessionsReply(request: Request): string {
  const first = request.days[0];
  if (first === undefined) {
    throw new Error('the request has no day');
  }
  const opened = Date.parse(first.start);
  const sessions = Array.from({ length: sessionCount }, (_, index) => {
    const start = new Date(opened + (index * 55 + 30) * 60_000);
    const minutes = 20 + ((index * 17) % 40);
    return {
      start: start.toISOString().replace('.000Z', 'Z'),
      end: new Date(start.getTime() + minutes * 60_000).toISOString().replace('.000Z', 'Z'),
      minutes,
      tool: index % 3 === 2 ? 'codex' : 'claude-code',
      project: request.group ?? 'billing',
      ticket: index % 2 === 0 ? 'alpha-2' : 'alpha-1',
      session: `a1b2c${index}d4`,
    };
  });
  return JSON.stringify({ total: sessionCount, next_offset: null, sessions });
}

function answerOf(agent: Agent): (line: string) => Promise<string> {
  return async (line) => {
    if (line.startsWith('dashboard-numbers ')) {
      await (agent.hold ?? Promise.resolve());
      return numbersReply(
        JSON.parse(line.slice('dashboard-numbers '.length)) as Request,
        agent.empty,
      );
    }
    if (line.startsWith('dashboard-sessions ')) {
      return sessionsReply(JSON.parse(line.slice('dashboard-sessions '.length)) as Request);
    }
    if (line.startsWith('dashboard-summary ')) {
      const { day } = JSON.parse(line.slice('dashboard-summary '.length)) as { day: Day };
      return JSON.stringify({
        day: day.label,
        lines: agent.empty ? [] : ['alpha-2 (billing): 1 h 20 min', 'alpha-1 (search): 45 min'],
      });
    }
    if (line === 'dashboard-get') {
      return JSON.stringify({ document });
    }
    if (line === 'settings-get') {
      return settingsReply(agent.collecting);
    }
    return JSON.stringify({ status: 'running', version: '0.1.0', pid: 42, model: 'example-model' });
  };
}

test('a walk through the dashboard, with a screenshot of each step and a recording of it', async ({
  session,
}) => {
  test.setTimeout(240_000);
  rmSync(videoDir, { recursive: true, force: true });
  mkdirSync(videoDir, { recursive: true });

  let release: () => void = () => undefined;
  const agent: Agent = {
    collecting: true,
    empty: false,
    hold: new Promise<void>((resolve) => {
      release = resolve;
    }),
  };

  await withStandIn(
    session,
    answerOf(agent),
    async (page, app) => {
      const total = page.locator('#value-total');
      const open = async (): Promise<void> => {
        await page.reload();
        await page.getByRole('button', { name: 'Dashboard', exact: true }).click();
      };
      /** A screenshot of the step in both themes, once the page has stood still for a moment. */
      const step = async (name: string): Promise<void> => {
        await page.waitForTimeout(linger);
        await screenshotBothThemes(page, `dashboard-tour-${name}`);
        await page.waitForTimeout(linger / 2);
      };

      await app.evaluate(({ BrowserWindow }, { width, height }) => {
        BrowserWindow.getAllWindows()[0]?.setContentSize(width, height);
      }, size);
      await page.clock.install({ time: now });

      // 1. The page says that it is reading the history, until the agent has the numbers.
      await open();
      await expect(page.getByText('Reading the history')).toBeVisible();
      await step('01-reading-history');
      release();

      // 2. The week: the total, the time of each day, and the time of each project and ticket.
      await expect(total).toBeVisible();
      await step('02-week');
      await page
        .getByRole('heading', { name: 'Time by project' })
        .evaluate((heading) => heading.scrollIntoView({ block: 'start' }));
      await step('03-by-project-and-ticket');
      await page.evaluate(() => window.scrollTo(0, 0));

      // 3. The week before, and then a chosen date in the week before that.
      const thisWeek = await total.innerText();
      await page.locator('#dashboard-previous-week').click();
      await expect(total).not.toHaveText(thisWeek);
      await step('04-previous-week');
      const lastWeek = await total.innerText();
      await page.locator('#dashboard-date').click();
      await page.waitForTimeout(linger);
      await page.getByRole('button', { name: /November 3rd,/ }).click();
      await expect(total).not.toHaveText(lastWeek);
      await step('05-chosen-date');

      // 4. Back to this week, and a click on a row: the sessions behind it.
      await page.locator('#dashboard-next-week').click();
      await page.locator('#dashboard-next-week').click();
      await expect(total).toHaveText(thisWeek);
      await page
        .locator('#table-by-project tbody tr', { hasText: 'search' })
        .locator('td')
        .nth(2)
        .click();
      await expect(page.getByRole('dialog', { name: 'Sessions of search' })).toBeVisible();
      await expect(page.locator('#drill-sessions tbody tr')).toHaveCount(sessionCount);
      await step('06-sessions-of-a-project');
      await page.keyboard.press('Escape');
      await expect(page.getByRole('dialog')).toHaveCount(0);

      // 5. The end-of-day summary of the day before, and its Copy.
      await page.locator('#dashboard-previous-day').click();
      const summary = page.locator('#summary-end-of-day');
      await expect(summary.locator('li')).toHaveCount(2);
      await summary.scrollIntoViewIfNeeded();
      await step('07-end-of-day');
      await page.locator('#summary-end-of-day-copy').click();
      await expect(summary.getByRole('status')).toHaveText('Copied');
      expect(await app.evaluate(({ clipboard }) => clipboard.readText())).toBe(
        'alpha-2 (billing): 1 h 20 min\nalpha-1 (search): 45 min',
      );
      await step('08-end-of-day-copied');

      // 6. Nothing to show, and collection off.
      agent.empty = true;
      await open();
      await expect(page.locator('#dashboard-empty')).toContainText('Nothing to show');
      await step('09-nothing-to-show');
      agent.collecting = false;
      await open();
      await expect(page.locator('#dashboard-off')).toContainText('Collection is off');
      await step('10-collection-off');
    },
    { recordVideo: { dir: videoDir, size } },
  );

  // The recording is complete once the window is closed.
  const [recording] = readdirSync(videoDir).filter((file) => file.endsWith('.webm'));
  if (recording === undefined) {
    throw new Error('the window was recorded, but there is no video');
  }
  renameSync(path.join(videoDir, recording), path.join(resultsDir, 'dashboard-tour.webm'));
  rmSync(videoDir, { recursive: true, force: true });
});
