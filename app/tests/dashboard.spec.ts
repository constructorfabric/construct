import type { ElectronApplication, Page } from '@playwright/test';
import { addDays, startOfDay } from 'date-fns';
import type { Whose } from '../src/dashboard-protocol';
import { dayLabel, formatMinutes, mondayOf } from '../src/ui/dashboard-period';
import { expect, test, withStandIn } from './fixtures';
import type { Session } from './fixtures';
import { firstCells, now, settingsReply } from './dashboard-shared';
import { screenshotBothThemes } from './screenshots';

interface Request {
  readonly days: { readonly label: string; readonly start: string; readonly end: string }[];
  readonly widget?: string;
  readonly sessions?: Whose;
  readonly filter?: Record<string, string[]>;
}

interface StandInWidget {
  readonly id: string;
  readonly kind: string;
  readonly group_by?: string;
  readonly period?: string;
  readonly sessions?: Whose;
  readonly position: { column: number; row: number; width: number; height: number };
}

type Document = {
  readonly type: string;
  readonly title: string;
  readonly payload: {
    readonly period: string;
    readonly sessions: Whose;
    readonly widgets: readonly StandInWidget[];
    readonly hidden_projects: readonly string[];
    readonly project_names: Record<string, string>;
  };
};

interface Problem {
  readonly kind: string;
  readonly place: string;
  readonly message: string;
}

/** What the stand-in agent answers, and what it has been asked. */
interface Agent {
  collecting: boolean;
  /** Minutes the stand-in finds in a period: none, or a fixed pattern. */
  empty: boolean;
  /** The last good document, and why the file as it is now is not used. */
  document: Document | null;
  problem: Problem | null;
  /** The agent's error text for the document, when it cannot give it. */
  documentError: string | null;
  /** While set, the numbers wait for it: the page is reading. */
  hold: Promise<void> | null;
  /** The agent's error text for the numbers, when it cannot give them. */
  numbersError: string | null;
  /** The agent's error text for the settings, when it cannot give them. */
  settingsError: string | null;
  /** How many times the agent says it is still reading the history before it has numbers. */
  building: number;
  /** How many times the page has asked for the document. */
  looks: number;
  readonly requests: Request[];
}

const gridPlaces = (widgets: readonly StandInWidget[]): Document => ({
  type: 'a dashboard type',
  title: 'Activity',
  payload: {
    period: 'this_week',
    sessions: 'own',
    widgets,
    hidden_projects: [],
    project_names: {},
  },
});

const widget = (
  id: string,
  kind: string,
  at: [number, number, number, number],
  more: Partial<StandInWidget> = {},
): StandInWidget => ({
  id,
  kind,
  position: { column: at[0], row: at[1], width: at[2], height: at[3] },
  ...more,
});

/** The dashboard the agent writes when there is none. */
const defaultDocument = gridPlaces([
  widget('total', 'total', [0, 0, 12, 2]),
  widget('per-day', 'bars_per_day', [0, 2, 12, 4]),
  widget('by-project', 'table', [0, 6, 6, 5], { group_by: 'project' }),
  widget('by-ticket', 'table', [6, 6, 6, 5], { group_by: 'ticket' }),
  widget('end-of-day', 'day_summary', [0, 11, 12, 3]),
]);

/** Monday 10 min, Tuesday 20 min ... Sunday 70 min: the user's own time. */
function ownMinutes(index: number): number {
  return (index + 1) * 10;
}

/** The unattended agents worked 30 minutes, on the first day of the period only. */
function unattendedMinutes(index: number): number {
  return index === 0 ? 30 : 0;
}

function minutesOf(whose: Whose, index: number): number {
  const own = whose === 'unattended' ? 0 : ownMinutes(index);
  const unattended = whose === 'own' ? 0 : unattendedMinutes(index);
  return own + unattended;
}

/** The groups of a table, in no order: the page sorts them. */
const groupsOf: Record<string, { key: string; minutes: number; sessions: number }[]> = {
  project: [
    { key: 'billing', minutes: 40, sessions: 2 },
    { key: 'search', minutes: 120, sessions: 5 },
    { key: 'docs', minutes: 20, sessions: 1 },
  ],
  ticket: [
    { key: 'No ticket', minutes: 15, sessions: 1 },
    { key: 'alpha-2', minutes: 90, sessions: 3 },
    { key: 'alpha-1', minutes: 45, sessions: 2 },
  ],
  tool: [
    { key: 'claude', minutes: 30, sessions: 4 },
    { key: 'codex', minutes: 60, sessions: 2 },
  ],
};

function numbersReply(agent: Agent, request: Request): string {
  if (agent.building > 0) {
    agent.building -= 1;
    return JSON.stringify({ error: 'the activity time is being built' });
  }
  if (agent.numbersError !== null) {
    return JSON.stringify({ error: agent.numbersError });
  }
  const found = agent.document?.payload.widgets.find((w) => w.id === request.widget);
  const whose =
    request.sessions ?? found?.sessions ?? agent.document?.payload.sessions ?? ('own' as const);
  const days = request.days.map((day, index) => ({
    label: day.label,
    minutes: agent.empty ? 0 : minutesOf(whose, index),
  }));
  const total = days.reduce((sum, day) => sum + day.minutes, 0);
  const rows =
    found?.kind === 'table' && !agent.empty
      ? (groupsOf[found.group_by ?? 'project'] ??
        days.map((day) => ({ key: day.label, minutes: day.minutes, sessions: 1 })))
      : [];
  const sum = rows.reduce((all, row) => all + row.minutes, 0);
  return JSON.stringify({
    total_minutes: total,
    session_count: days.filter((day) => day.minutes > 0).length,
    hidden_project_count: 0,
    groups: rows.map(({ sessions, ...row }) => ({
      ...row,
      session_count: sessions,
      share: row.minutes / sum,
    })),
    days,
  });
}

function agentSettingsReply(agent: Agent): string {
  if (agent.settingsError !== null) {
    return JSON.stringify({ error: agent.settingsError });
  }
  return settingsReply(agent.collecting);
}

/** Answers the window's calls with `agent` and opens the Dashboard page. */
async function withAgent(
  session: Session,
  agent: Agent,
  check: (page: Page, app: ElectronApplication) => Promise<void>,
  options: { readonly arrange?: boolean } = {},
): Promise<void> {
  await withStandIn(
    session,
    async (line) => {
      if (line.startsWith('dashboard-numbers ')) {
        const request = JSON.parse(line.slice('dashboard-numbers '.length)) as Request;
        agent.requests.push(request);
        await (agent.hold ?? Promise.resolve());
        return numbersReply(agent, request);
      }
      if (line.startsWith('dashboard-summary ')) {
        const day = (
          JSON.parse(line.slice('dashboard-summary '.length)) as { day: { label: string } }
        ).day.label;
        return JSON.stringify({
          day,
          total_minutes: agent.empty ? 0 : 30,
          lines: agent.empty ? [] : ['No ticket (billing): 30 min'],
        });
      }
      if (line === 'dashboard-get') {
        agent.looks += 1;
        if (agent.documentError !== null) {
          return JSON.stringify({ error: agent.documentError });
        }
        return JSON.stringify({
          document: agent.document,
          ...(agent.problem === null ? {} : { error: agent.problem }),
        });
      }
      if (line === 'settings-get') {
        return agentSettingsReply(agent);
      }
      return JSON.stringify({
        status: 'running',
        version: '0.1.0',
        pid: 42,
        model: 'example-model',
      });
    },
    async (page, app) => {
      await page.clock.install({ time: now });
      if (options.arrange === true) {
        // The page has no control for arranging yet; its address does.
        await page.goto(`${page.url().split('?')[0] ?? ''}?arrange`);
      } else {
        await page.reload();
      }
      await page.getByRole('button', { name: 'Dashboard', exact: true }).click();
      await check(page, app);
      await expectNoStyleElements(page);
    },
  );
}

/** The policy allows no `<style>` element, and the grid and the charts must not add one. */
async function expectNoStyleElements(page: Page): Promise<void> {
  expect(await page.evaluate(() => document.querySelectorAll('style').length)).toBe(0);
}

function newAgent(overrides: Partial<Agent> = {}): Agent {
  return {
    collecting: true,
    empty: false,
    document: defaultDocument,
    problem: null,
    documentError: null,
    hold: null,
    looks: 0,
    numbersError: null,
    settingsError: null,
    building: 0,
    requests: [],
    ...overrides,
  };
}

interface Box {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

async function boxOf(page: Page, id: string): Promise<Box> {
  const box = await page.locator(`#widget-${id}`).boundingBox();
  if (box === null) {
    throw new Error(`widget ${id} is not on the page`);
  }
  return box;
}

/** Where a widget sits on the grid, as the page reports it. */
async function placeOf(page: Page, id: string): Promise<number[]> {
  const widgetElement = page.locator(`#widget-${id}`);
  return Promise.all(
    ['column', 'row', 'width', 'height'].map(async (name) =>
      Number(await widgetElement.getAttribute(`data-${name}`)),
    ),
  );
}

const rowStep = 56 + 16;

/** The week total the stand-in gives for the user's own time. */
const ownWeek = formatMinutes([0, 1, 2, 3, 4, 5, 6].reduce((sum, i) => sum + ownMinutes(i), 0));

test('the default dashboard draws a total, bars and two tables where the document puts them', async ({
  session,
}) => {
  const agent = newAgent();
  await withAgent(session, agent, async (page) => {
    await expect(page.locator('#value-total')).toHaveText(ownWeek);
    await expect(page.locator('#widget-total h2')).toHaveText('Time');

    for (const widgetPlace of defaultDocument.payload.widgets) {
      const { column, row, width, height } = widgetPlace.position;
      expect(await placeOf(page, widgetPlace.id)).toEqual([column, row, width, height]);
    }
    await expect(page.locator('#widget-end-of-day h2')).toHaveText('End of day');

    // On the screen: the total is as wide as the grid, the bars sit two rows below it, and the
    // two tables share the width of the row under them.
    const [total, bars, projects, tickets] = await Promise.all(
      ['total', 'per-day', 'by-project', 'by-ticket'].map((id) => boxOf(page, id)),
    );
    if (
      total === undefined ||
      bars === undefined ||
      projects === undefined ||
      tickets === undefined
    ) {
      throw new Error('a widget has no box');
    }
    expect(bars.width).toBeCloseTo(total.width, 0);
    expect(bars.x).toBeCloseTo(total.x, 0);
    expect(bars.y - total.y).toBeCloseTo(2 * rowStep, 0);
    expect(bars.height).toBeCloseTo(4 * rowStep - 16, 0);
    expect(projects.y - bars.y).toBeCloseTo(4 * rowStep, 0);
    expect(tickets.y).toBeCloseTo(projects.y, 0);
    expect(projects.x).toBeCloseTo(total.x, 0);
    expect(projects.width * 2 + 16).toBeCloseTo(total.width, 0);
    expect(tickets.x - projects.x).toBeCloseTo(projects.width + 16, 0);

    // The bars have a text alternative: a table of the days.
    const days = page.locator('#chart-per-day table tbody tr');
    await expect(days).toHaveCount(7);
    await expect(days.first().locator('td')).toHaveText(formatMinutes(ownMinutes(0)));
    await expect(page.locator('#chart-per-day svg .recharts-bar-rectangle').first()).toBeVisible();

    // The window asks for each widget by its id, with the days of its period.
    const today = startOfDay(now);
    const asked = agent.requests.find((request) => request.widget === 'by-project');
    expect(asked?.days.map((day) => day.label)).toEqual(
      [0, 1, 2, 3, 4, 5, 6].map((i) => dayLabel(addDays(mondayOf(today), i))),
    );
    await screenshotBothThemes(page, 'dashboard-widgets');
    await page
      .getByRole('heading', { name: 'Time by project' })
      .evaluate((heading) => heading.scrollIntoView({ block: 'start' }));
    await screenshotBothThemes(page, 'dashboard-widgets-tables');
  });
});

test('each table is sorted by time, longest first', async ({ session }) => {
  await withAgent(session, newAgent(), async (page) => {
    await expect(page.locator('#table-by-project')).toBeVisible();
    expect(await firstCells(page, 'by-project')).toEqual(['search', 'billing', 'docs']);
    expect(await firstCells(page, 'by-ticket')).toEqual(['alpha-2', 'alpha-1', 'No ticket']);

    const rows = page.locator('#table-by-project tbody tr');
    await expect(rows.first().locator('td').nth(1)).toHaveText('2 h 0 min');
    await expect(rows.first().locator('td').nth(2)).toContainText('67%');
    await expect(rows.first().locator('td').nth(3)).toHaveText('5');
    // The share is a bar and a number, so it does not rest on colour.
    await expect(rows.first().getByRole('progressbar')).toHaveAttribute('aria-valuenow', '67');
    await expect(rows.last().locator('td').nth(1)).toHaveText('20 min');
  });
});

test('a document with other positions draws the widgets there', async ({ session }) => {
  const other = gridPlaces([
    widget('by-tool', 'table', [0, 0, 5, 4], { group_by: 'tool' }),
    widget('total', 'total', [7, 1, 5, 2], { period: 'today' }),
    widget('per-day', 'bars_per_day', [2, 5, 10, 3], { period: 'last_7_days' }),
  ]);
  await withAgent(session, newAgent({ document: other }), async (page, app) => {
    await expect(page.locator('#table-by-tool')).toBeVisible();
    expect(await placeOf(page, 'by-tool')).toEqual([0, 0, 5, 4]);
    expect(await placeOf(page, 'total')).toEqual([7, 1, 5, 2]);
    expect(await placeOf(page, 'per-day')).toEqual([2, 5, 10, 3]);
    expect(await firstCells(page, 'by-tool')).toEqual(['codex', 'claude']);

    // A gap above a widget is kept: nothing slides up to close it.
    const tool = await boxOf(page, 'by-tool');
    const total = await boxOf(page, 'total');
    const bars = await boxOf(page, 'per-day');
    expect(total.y - tool.y).toBeCloseTo(rowStep, 0);
    expect(total.x).toBeGreaterThan(tool.x + tool.width);
    expect(bars.y - tool.y).toBeCloseTo(5 * rowStep, 0);
    expect(bars.x - tool.x).toBeGreaterThan(0);
    // The widget's own period: today alone for the total.
    await expect(page.locator('#value-total')).toHaveText(formatMinutes(ownMinutes(0)));
    await screenshotBothThemes(page, 'dashboard-widgets-other-places');

    // The grid follows the window: a narrower window makes every widget narrower.
    const before = await boxOf(page, 'per-day');
    await app.evaluate(({ BrowserWindow }) => {
      BrowserWindow.getAllWindows()[0]?.setSize(720, 640);
    });
    await expect.poll(async () => (await boxOf(page, 'per-day')).width).toBeLessThan(before.width);
    // Both widgets end at the right edge of the grid again once the grid has settled.
    await expect
      .poll(async () => {
        const narrow = await boxOf(page, 'per-day');
        const narrowTotal = await boxOf(page, 'total');
        return Math.abs(narrow.x + narrow.width - (narrowTotal.x + narrowTotal.width)) < 1;
      })
      .toBe(true);
  });
});

test('a bars widget that counts both shows the own and the unattended time apart', async ({
  session,
}) => {
  const both = gridPlaces([widget('per-day', 'bars_per_day', [0, 0, 12, 4], { sessions: 'all' })]);
  await withAgent(session, newAgent({ document: both }), async (page) => {
    const days = page.locator('#chart-per-day table tbody tr');
    await expect(days).toHaveCount(7);
    // Both kinds are named in the legend and in the table, not only coloured.
    await expect(page.locator('#chart-per-day li')).toHaveText(['Your time', 'Unattended time']);
    await expect(days.first().locator('td')).toHaveText(['10 min', '30 min']);
    await expect(days.nth(1).locator('td')).toHaveText(['20 min', '0 min']);
    await screenshotBothThemes(page, 'dashboard-widgets-split');
  });
});

test('a broken document shows its error over the last good dashboard', async ({ session }) => {
  const agent = newAgent();
  await withAgent(session, agent, async (page) => {
    await expect(page.locator('#value-total')).toHaveText(ownWeek);
    await expect(page.locator('#dashboard-problem')).toHaveCount(0);

    agent.problem = {
      kind: 'pointer',
      place: '/payload/widgets/1/position/width',
      message: 'is greater than the maximum of 12',
    };
    const problem = page.locator('#dashboard-problem');
    await expect(problem).toContainText('/payload/widgets/1/position/width');
    await expect(problem).toContainText('is greater than the maximum of 12');
    await expect(problem).toContainText('The last good dashboard is shown.');
    // The widgets of the last good document stay where they were.
    await expect(page.locator('#value-total')).toHaveText(ownWeek);
    expect(await placeOf(page, 'by-ticket')).toEqual([6, 6, 6, 5]);
    await screenshotBothThemes(page, 'dashboard-widgets-problem');

    agent.problem = {
      kind: 'line',
      place: 'line 3, column 7',
      message: 'the file is not valid JSON',
    };
    await expect(problem).toContainText('line 3, column 7');

    // A file that passes again takes the error away.
    agent.problem = null;
    await expect(problem).toHaveCount(0);
    await expect(page.locator('#value-total')).toHaveText(ownWeek);
  });
});

test('a file that never passed shows the error and no widgets', async ({ session }) => {
  const agent = newAgent({
    document: null,
    problem: { kind: 'line', place: 'line 1, column 1', message: 'the file is not valid JSON' },
  });
  await withAgent(session, agent, async (page) => {
    await expect(page.locator('#dashboard-problem')).toContainText('line 1, column 1');
    await expect(page.locator('#dashboard-problem')).not.toContainText('last good');
    await expect(page.locator('[id^="widget-"]')).toHaveCount(0);
  });
});

test('a failed look at the file keeps the last good dashboard', async ({ session }) => {
  const agent = newAgent();
  await withAgent(session, agent, async (page) => {
    await expect(page.locator('#value-total')).toBeVisible();
    agent.documentError = 'the dashboard is not available';
    // The page asks again every two seconds: wait for two looks that fail, then check.
    const failed = agent.looks;
    await expect.poll(() => agent.looks, { timeout: 15_000 }).toBeGreaterThanOrEqual(failed + 2);
    await expect(page.locator('#value-total')).toBeVisible();
    await expect(page.locator('#dashboard-failed')).toHaveCount(0);
  });
});

test('a document with nothing to draw says so', async ({ session }) => {
  const agent = newAgent({ document: gridPlaces([]) });
  await withAgent(session, agent, async (page) => {
    await expect(page.locator('#dashboard-no-widgets')).toContainText('Nothing to draw');
  });
});

test('a document with only the summary of the day draws it', async ({ session }) => {
  const agent = newAgent({
    document: gridPlaces([widget('end-of-day', 'day_summary', [0, 0, 12, 3])]),
  });
  await withAgent(session, agent, async (page) => {
    await expect(page.locator('#widget-end-of-day h2')).toHaveText('End of day');
    await expect(page.locator('#dashboard-no-widgets')).toHaveCount(0);
  });
});

test('a new document replaces the widgets as the file changes', async ({ session }) => {
  const agent = newAgent();
  await withAgent(session, agent, async (page) => {
    await expect(page.locator('#table-by-project')).toBeVisible();
    agent.document = gridPlaces([widget('total', 'total', [3, 0, 6, 2])]);
    await expect(page.locator('#widget-by-project')).toHaveCount(0);
    expect(await placeOf(page, 'total')).toEqual([3, 0, 6, 2]);
  });
});

test('the arrows and the date picker change the day the periods are counted from', async ({
  session,
}) => {
  const agent = newAgent();
  await withAgent(session, agent, async (page) => {
    const today = startOfDay(now);
    const firstDay = (): string | undefined =>
      agent.requests.filter((request) => request.widget === 'total').at(-1)?.days[0]?.label;
    await expect.poll(firstDay).toBe(dayLabel(mondayOf(today)));

    await page.getByRole('button', { name: 'Next week', exact: true }).click();
    await expect.poll(firstDay).toBe(dayLabel(mondayOf(addDays(today, 7))));
    await page.getByRole('button', { name: 'Previous week', exact: true }).click();
    await expect.poll(firstDay).toBe(dayLabel(mondayOf(today)));

    // A day at a time: seven steps back always leave the week before.
    for (let step = 0; step < 7; step += 1) {
      await page.getByRole('button', { name: 'Previous day', exact: true }).click();
    }
    await expect.poll(firstDay).toBe(dayLabel(mondayOf(addDays(today, -7))));
    for (let step = 0; step < 7; step += 1) {
      await page.getByRole('button', { name: 'Next day', exact: true }).click();
    }
    await expect.poll(firstDay).toBe(dayLabel(mondayOf(today)));

    // Any date: the 10th of November, in the week before the one shown.
    const target = new Date(2026, 10, 10);
    await page.locator('#dashboard-date').click();
    await page.getByRole('button', { name: /November 10th,/ }).click();
    await expect.poll(firstDay).toBe(dayLabel(mondayOf(target)));
  });
});

/** A page of widgets that count a week at the most. */
const weeklyDocument = gridPlaces([
  widget('total', 'total', [0, 0, 12, 2]),
  widget('per-day', 'bars_per_day', [0, 2, 12, 4], { period: 'last_week' }),
]);

test('with weekly widgets only, the arrows step by a week and the date picker jumps to a week', async ({
  session,
}) => {
  const agent = newAgent({ document: weeklyDocument });
  await withAgent(session, agent, async (page) => {
    const today = startOfDay(now);
    const monday = mondayOf(today);
    const firstDay = (): string | undefined =>
      agent.requests.filter((request) => request.widget === 'total').at(-1)?.days[0]?.label;
    const header = page.locator('#dashboard-period');
    await expect.poll(firstDay).toBe(dayLabel(monday));
    await expect(header).toHaveText('Nov 16 – Nov 22, 2026');
    await expect(page.locator('#dashboard-previous-day')).toHaveCount(0);
    await expect(page.locator('#dashboard-next-day')).toHaveCount(0);

    await page.locator('#dashboard-previous-week').click();
    await expect.poll(firstDay).toBe(dayLabel(addDays(monday, -7)));
    await expect(header).toHaveText('Nov 9 – Nov 15, 2026');
    await page.locator('#dashboard-next-week').click();
    await page.locator('#dashboard-next-week').click();
    await expect.poll(firstDay).toBe(dayLabel(addDays(monday, 7)));
    await expect(header).toHaveText('Nov 23 – Nov 29, 2026');

    // A date in the middle of a week: the page shows that week.
    await page.locator('#dashboard-date').click();
    await page.getByRole('button', { name: /November 4th,/ }).click();
    await expect.poll(firstDay).toBe('2026-11-02');
    await expect(header).toHaveText('Nov 2 – Nov 8, 2026');

    // Each widget names its days: the week of the page, and the week before it.
    await expect(page.locator('#widget-total')).toContainText('This week · Nov 2 – Nov 8');
    await expect(page.locator('#widget-per-day')).toContainText('Last week · Oct 26 – Nov 1');
    await screenshotBothThemes(page, 'dashboard-period-week');
  });
});

test('with a widget that counts a day, the arrows step by a day and the header names the day', async ({
  session,
}) => {
  const agent = newAgent();
  await withAgent(session, agent, async (page) => {
    const header = page.locator('#dashboard-period');
    await expect(header).toHaveText('Nov 18, 2026');
    await page.locator('#dashboard-previous-day').click();
    await expect(header).toHaveText('Nov 17, 2026');
    await expect(page.locator('#widget-end-of-day')).toContainText('2026-11-17');
    await expect(page.locator('#widget-total')).toContainText('This week · Nov 16 – Nov 22');
    await page.locator('#dashboard-next-day').click();
    await page.locator('#dashboard-next-day').click();
    await expect(header).toHaveText('Nov 19, 2026');
    await expect(page.locator('#widget-end-of-day')).toContainText('2026-11-19');
    await screenshotBothThemes(page, 'dashboard-period-day');
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
    await expect(page.locator('#value-total')).toHaveCount(0);
    await screenshotBothThemes(page, 'dashboard-reading');
    release();
    await expect(page.locator('#value-total')).toHaveText(ownWeek);
    await expect(page.getByText('Reading the history')).toHaveCount(0);
  });
});

test('the page says when there is nothing to show', async ({ session }) => {
  await withAgent(session, newAgent({ empty: true }), async (page) => {
    await expect(page.locator('#dashboard-empty')).toContainText('Nothing to show');
    await expect(page.locator('#value-total')).toHaveCount(0);
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

test('with dragging and resizing on, a widget moves and another one grows', async ({ session }) => {
  // Three widgets that fit the window without scrolling. The total has a free row to cross, and
  // the table has room to grow: a widget dragged through another one pushes it down.
  const document = gridPlaces([
    widget('total', 'total', [0, 0, 4, 2]),
    widget('by-project', 'table', [4, 2, 4, 3], { group_by: 'project' }),
    widget('by-tool', 'table', [0, 2, 4, 3], { group_by: 'tool' }),
  ]);
  await withAgent(
    session,
    newAgent({ document }),
    async (page) => {
      await expect(page.locator('#table-by-project')).toBeVisible();
      expect(await placeOf(page, 'total')).toEqual([0, 0, 4, 2]);
      // Four columns and the three gaps between them are as wide as the total, so a column and
      // its gap are a quarter of that and a gap.
      const total = await boxOf(page, 'total');
      const step = (total.width + 16) / 4;

      // Drag the total to the right edge of the grid, eight columns on.
      const grab = { x: total.x + total.width / 2, y: total.y + total.height / 2 };
      await page.mouse.move(grab.x, grab.y);
      await page.mouse.down();
      await page.mouse.move(grab.x + step * 3, grab.y, { steps: 6 });
      await page.mouse.move(grab.x + step * 8, grab.y, { steps: 12 });
      await page.mouse.up();
      await expect.poll(() => placeOf(page, 'total')).toEqual([8, 0, 4, 2]);
      await expect
        .poll(async () => (await boxOf(page, 'total')).x)
        .toBeGreaterThan(total.x + step * 7);

      // Resize the table from its corner: two columns wider and one row taller.
      const table = await boxOf(page, 'by-project');
      const handle = page.locator('#widget-by-project .react-resizable-handle-se');
      await expect(handle).toBeVisible();
      const corner = await handle.boundingBox();
      if (corner === null) {
        throw new Error('the resize handle has no box');
      }
      const from = { x: corner.x + corner.width / 2, y: corner.y + corner.height / 2 };
      await page.mouse.move(from.x, from.y);
      await page.mouse.down();
      await page.mouse.move(from.x + step, from.y + 10, { steps: 6 });
      await page.mouse.move(from.x + step * 2, from.y + rowStep, { steps: 12 });
      await page.mouse.up();
      await expect.poll(() => placeOf(page, 'by-project')).toEqual([4, 2, 6, 4]);
      await expect
        .poll(async () => (await boxOf(page, 'by-project')).width)
        .toBeGreaterThan(table.width + step);
      expect((await boxOf(page, 'by-project')).height).toBeGreaterThan(table.height + rowStep / 2);
      // The widget that was not touched is where it was.
      expect(await placeOf(page, 'by-tool')).toEqual([0, 2, 4, 3]);
      await screenshotBothThemes(page, 'dashboard-widgets-arranged');
    },
    { arrange: true },
  );
});

test('without the switch, a widget stays where it is when it is dragged', async ({ session }) => {
  await withAgent(session, newAgent(), async (page) => {
    await expect(page.locator('#table-by-project')).toBeVisible();
    const total = await boxOf(page, 'total');
    const grab = { x: total.x + 40, y: total.y + total.height / 2 };
    await page.mouse.move(grab.x, grab.y);
    await page.mouse.down();
    await page.mouse.move(grab.x + 200, grab.y + 80, { steps: 8 });
    await page.mouse.up();
    expect(await placeOf(page, 'total')).toEqual([0, 0, 12, 2]);
    await expect(page.locator('#widget-total .react-resizable-handle')).toBeHidden();
  });
});

test('with collection off, the numbers of the past still show, with a note and the link', async ({
  session,
}) => {
  await withAgent(session, newAgent({ collecting: false }), async (page) => {
    await expect(page.locator('#value-total')).toHaveText(ownWeek);
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
    // Each widget shows the agent's words in place of its numbers.
    for (const id of ['total', 'per-day', 'by-project', 'by-ticket']) {
      await expect(page.locator(`#widget-${id}`)).toContainText(
        `The dashboard is not available: ${error}`,
      );
    }
    await expect(page.locator('#value-total')).toHaveCount(0);
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

test('the page reads the numbers of the real agent: an empty week says nothing to show', async ({
  page,
}) => {
  // The first start of the agent unpacks its database, which is longer than the default timeout.
  test.setTimeout(150_000);
  await page.getByRole('button', { name: 'Dashboard', exact: true }).click();
  await expect(page.locator('#dashboard-empty')).toContainText('Nothing to show', {
    timeout: 120_000,
  });
  await expect(page.locator('#dashboard-failed')).toHaveCount(0);
});

test('the page keeps saying it is reading the history until the agent has the numbers', async ({
  session,
}) => {
  const agent = newAgent({ building: 2 });
  await withAgent(session, agent, async (page) => {
    await expect(page.getByText('Reading the history')).toBeVisible();
    await expect(page.locator('#dashboard-failed')).toHaveCount(0);
    await expect(page.locator('#value-total')).toHaveText(ownWeek, { timeout: 15_000 });
    // Both answers of "still reading" were given before the numbers came.
    expect(agent.building).toBe(0);
  });
});
