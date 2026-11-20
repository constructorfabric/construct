import type { ElectronApplication, Page } from '@playwright/test';
import { formatMinutes } from '../src/ui/dashboard-period';
import type { GrantState } from '../src/screen-grants';
import { firstCells, gridPlaces, now, settingsReply, widget } from './dashboard-shared';
import { expect, gate, test, windowText, withStandIn } from './fixtures';
import type { Session } from './fixtures';
import { screenshotBothThemes } from './screenshots';

/** The window runs in a zone that is not UTC, so a time shown unconverted does not match. */
const zone = 'America/New_York';

interface Request {
  readonly days: readonly { readonly label: string }[];
  readonly widget?: string;
  readonly source?: string;
  readonly sessions?: string;
  readonly group_by?: string;
  readonly group?: string;
  readonly offset?: number;
  readonly limit?: number;
}

/** What the stand-in agent answers, and what it has been asked. */
interface Agent {
  /** Whether the person collects the time on screen. */
  screen: boolean;
  /** Whether this computer can collect it at all. */
  supported: boolean;
  grants: { name: string; state: GrantState }[];
  /** No time on screen in any period. */
  empty: boolean;
  /** While set, the numbers wait for it: the page is reading. */
  hold: Promise<void> | null;
  readonly numbers: Request[];
  readonly blocks: Request[];
}

/**
 * The AI sessions on top, as the default document has them, and the time on screen below. The
 * page counts `all` sessions, so a bars widget that was not told its source would be split.
 */
const document = gridPlaces(
  [
    widget('total', 'total', [0, 0, 12, 2]),
    widget('by-project', 'table', [0, 2, 12, 5], { group_by: 'project' }),
    widget('screen-total', 'total', [0, 14, 12, 2], { source: 'on_screen' }),
    widget('screen-per-day', 'bars_per_day', [0, 16, 12, 4], { source: 'on_screen' }),
    widget('screen-by-app', 'table', [0, 20, 6, 5], { source: 'on_screen', group_by: 'app' }),
    widget('screen-by-site', 'table', [6, 20, 6, 5], { source: 'on_screen', group_by: 'site' }),
    widget('screen-by-project', 'table', [0, 25, 6, 5], {
      source: 'on_screen',
      group_by: 'project',
    }),
    widget('screen-by-ticket', 'table', [6, 25, 6, 5], {
      source: 'on_screen',
      group_by: 'ticket',
    }),
    widget('screen-by-day', 'table', [0, 30, 12, 5], { source: 'on_screen', group_by: 'day' }),
  ],
  'all',
);

/** Minutes of the AI sessions of the week, and of the time on screen: two numbers, never added. */
const aiMinutes = 140;
const screenSeconds = 3 * 3600 + 20 * 60;
const screenMinutes = screenSeconds / 60;

/** The groups of the time on screen, in no order: the page sorts them. Seconds, as the blocks are. */
const screenGroups: Record<string, { key: string; seconds: number; blocks: number }[]> = {
  app: [
    { key: 'Terminal', seconds: 1800, blocks: 25 },
    { key: 'Safari', seconds: 7200, blocks: 4 },
    { key: 'Xcode', seconds: 3000, blocks: 2 },
  ],
  site: [
    { key: 'No site', seconds: 6000, blocks: 4 },
    { key: 'example.org', seconds: 6000, blocks: 3 },
  ],
  project: [
    { key: 'billing', seconds: 9000, blocks: 6 },
    { key: 'No project', seconds: 3000, blocks: 3 },
  ],
  ticket: [
    { key: 'No ticket', seconds: 4200, blocks: 4 },
    { key: 'alpha-7', seconds: 7800, blocks: 5 },
  ],
  day: [
    { key: '2026-11-16', seconds: 4800, blocks: 4 },
    { key: '2026-11-17', seconds: 7200, blocks: 5 },
  ],
};

function numbersReply(agent: Agent, request: Request): string {
  agent.numbers.push(request);
  const found = document.payload.widgets.find((one) => one.id === request.widget);
  const screen = found?.source === 'on_screen';
  const days = request.days.map((day, index) => ({
    label: day.label,
    minutes: agent.empty && screen ? 0 : screen ? (index === 0 ? 80 : index === 1 ? 120 : 0) : 20,
  }));
  const rows =
    screen && found?.kind === 'table' && !agent.empty
      ? (screenGroups[String(found.group_by)] ?? [])
      : [];
  const sum = rows.reduce((all, row) => all + row.seconds, 0);
  const total = screen ? (agent.empty ? 0 : screenMinutes) : aiMinutes;
  return JSON.stringify({
    ...(screen ? { source: 'on_screen' } : {}),
    total_minutes: total,
    session_count: total === 0 ? 0 : 1,
    hidden_project_count: 0,
    groups: rows.map((row) => ({
      key: row.key,
      minutes: row.seconds / 60,
      share: row.seconds / sum,
      session_count: row.blocks,
    })),
    days,
  });
}

const cycle = {
  app: ['Safari', 'Xcode', 'Terminal'],
  site: ['example.org', 'No site'],
  project: ['billing', 'No project'],
  ticket: ['alpha-7', 'No ticket'],
} as const;

/**
 * The blocks of a group, a page of them. The seconds of the blocks add up to the seconds of the
 * group, and no two neighbours are alike in any column, so a shifted column shows.
 */
function blocksReply(agent: Agent, request: Request): string {
  agent.blocks.push(request);
  const grouped =
    request.group_by ??
    String(document.payload.widgets.find((one) => one.id === request.widget)?.group_by);
  const group = (screenGroups[grouped] ?? []).find((one) => one.key === request.group);
  const seconds = group?.seconds ?? 0;
  const count = group?.blocks ?? 0;
  const length = (index: number): number => {
    if (index < count - 1) {
      return Math.floor(seconds / count) + (index % 2 === 0 ? 7 : -7);
    }
    return (
      seconds - Array.from({ length: count - 1 }, (_, i) => length(i)).reduce((a, b) => a + b, 0)
    );
  };
  const all = Array.from({ length: count }, (_, index) => {
    const start = Date.UTC(2026, 10, 16, 9 + index);
    const pick = (name: keyof typeof cycle): string =>
      grouped === name ? String(request.group) : (cycle[name][index % cycle[name].length] ?? '');
    return {
      start: new Date(start).toISOString(),
      end: new Date(start + length(index) * 1000).toISOString(),
      seconds: length(index),
      app: pick('app'),
      site: pick('site'),
      project: pick('project'),
      ticket: pick('ticket'),
      // A title is never part of a block; a stand-in that sent one shows the page does not draw it.
      title: 'secret window title',
    };
  });
  const offset = request.offset ?? 0;
  const end = Math.min(offset + (request.limit ?? 20), count);
  return JSON.stringify({
    source: 'on_screen',
    group: request.group,
    total: count,
    offset,
    limit: request.limit,
    next_offset: end < count ? end : null,
    blocks: all.slice(offset, end),
  });
}

function newAgent(overrides: Partial<Agent> = {}): Agent {
  return {
    screen: true,
    supported: true,
    grants: [
      { name: 'accessibility', state: 'granted' },
      { name: 'screen_recording', state: 'granted' },
      { name: 'automation', state: 'granted' },
      { name: 'input_monitoring', state: 'granted' },
    ],
    empty: false,
    hold: null,
    numbers: [],
    blocks: [],
    ...overrides,
  };
}

async function withAgent(
  session: Session,
  agent: Agent,
  check: (page: Page, app: ElectronApplication) => Promise<void>,
): Promise<void> {
  await withStandIn(
    { ...session, env: { ...session.env, TZ: zone } },
    async (line) => {
      if (line.startsWith('dashboard-numbers ')) {
        const request = JSON.parse(line.slice('dashboard-numbers '.length)) as Request;
        await (agent.hold ?? Promise.resolve());
        return numbersReply(agent, request);
      }
      if (line.startsWith('dashboard-sessions ')) {
        return blocksReply(agent, JSON.parse(line.slice('dashboard-sessions '.length)) as Request);
      }
      if (line.startsWith('dashboard-summary ')) {
        return JSON.stringify({ lines: [] });
      }
      if (line === 'dashboard-get') {
        return JSON.stringify({ document });
      }
      if (line === 'settings-get') {
        return settingsReply(true, { supported: agent.supported, on: agent.screen });
      }
      return JSON.stringify({
        status: 'running',
        version: '0.1.0',
        pid: 42,
        model: 'example-model',
        screen: { state: agent.screen ? 'running' : 'off', samples_today: 0, grants: agent.grants },
      });
    },
    async (page, app) => {
      await page.clock.install({ time: now });
      await page.reload();
      await page.getByRole('button', { name: 'Dashboard', exact: true }).click();
      await check(page, app);
    },
  );
}

const choose = async (page: Page, name: 'AI sessions' | 'On screen'): Promise<void> => {
  await page.getByRole('button', { name, exact: true }).click();
};

test('the page names the two sources and shows one at a time, never added', async ({ session }) => {
  const agent = newAgent();
  await withAgent(session, agent, async (page) => {
    const ai = page.getByRole('button', { name: 'AI sessions', exact: true });
    const screen = page.getByRole('button', { name: 'On screen', exact: true });
    await expect(ai).toHaveAttribute('aria-pressed', 'true');
    await expect(screen).toHaveAttribute('aria-pressed', 'false');

    // The AI sessions are what the page opens on; the card says what it counts.
    await expect(page.locator('#value-total')).toHaveText(formatMinutes(aiMinutes));
    await expect(page.locator('#widget-total')).toContainText('AI sessions');
    await expect(page.locator('#widget-screen-total')).toHaveCount(0);

    await choose(page, 'On screen');
    await expect(screen).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator('#value-screen-total')).toHaveText(formatMinutes(screenMinutes));
    await expect(page.locator('#widget-screen-total')).toContainText('On screen');
    await expect(page.locator('#value-total')).toHaveCount(0);
    // The first widget of the source is at the top of the grid, not under the other source's gap.
    await expect(page.locator('#widget-screen-total')).toHaveAttribute('data-row', '0');

    // Nothing on the page is the two numbers together.
    expect(await windowText(page)).not.toContain(formatMinutes(aiMinutes + screenMinutes));

    // No call names `sessions`: the bars of the time on screen are not split.
    expect(agent.numbers.some((request) => request.widget === 'screen-per-day')).toBe(true);
    expect(agent.numbers.filter((request) => request.sessions !== undefined)).toEqual([]);
    await screenshotBothThemes(page, 'dashboard-screen');

    await choose(page, 'AI sessions');
    await expect(page.locator('#value-total')).toHaveText(formatMinutes(aiMinutes));
  });
});

test('a table groups the time on screen by app, website, project, ticket and day', async ({
  session,
}) => {
  await withAgent(session, newAgent(), async (page) => {
    await choose(page, 'On screen');
    await expect
      .poll(() => firstCells(page, 'screen-by-app'))
      .toEqual(['Safari', 'Xcode', 'Terminal']);
    await expect(page.locator('#widget-screen-by-app h2')).toHaveText('Time by app');
    await expect(page.locator('#table-screen-by-app thead th').first()).toHaveText('App');
    // The count column is of blocks, not of sessions.
    await expect(page.locator('#table-screen-by-app thead th').nth(3)).toHaveText('Blocks');
    const safari = page.locator('#table-screen-by-app tbody tr', { hasText: 'Safari' });
    await expect(safari.locator('td').nth(1)).toHaveText('2 h 0 min');
    await expect(safari.locator('td').nth(2)).toContainText('60%');
    await expect(safari.locator('td').nth(3)).toHaveText('4');

    await expect.poll(() => firstCells(page, 'screen-by-site')).toEqual(['example.org', 'No site']);
    await expect(page.locator('#widget-screen-by-site h2')).toHaveText('Time by website');
    await expect(page.locator('#table-screen-by-site thead th').first()).toHaveText('Website');
    await expect
      .poll(() => firstCells(page, 'screen-by-project'))
      .toEqual(['billing', 'No project']);
    await expect.poll(() => firstCells(page, 'screen-by-ticket')).toEqual(['alpha-7', 'No ticket']);
    await expect
      .poll(() => firstCells(page, 'screen-by-day'))
      .toEqual(['2026-11-17', '2026-11-16']);

    // The bars are drawn by Recharts, one series named On screen, with the table of the same numbers.
    await expect(
      page.locator('#chart-screen-per-day svg .recharts-bar-rectangle').first(),
    ).toBeVisible();
    await expect(page.locator('#chart-screen-per-day')).toContainText('On screen');
    await expect(page.locator('#chart-screen-per-day table tbody tr')).toHaveCount(7);
    await expect(page.locator('#chart-screen-per-day table tbody tr td').first()).toHaveText(
      formatMinutes(80),
    );
  });
});

/** The seconds of a time as the dialog writes it: `1 h 2 min 5 s`, `5 min 20 s`, `45 s`. */
function secondsOf(text: string): number {
  const [, hours, minutes, rest] = /^(?:(\d+) h )?(?:(\d+) min )?(\d+) s$/.exec(text.trim()) ?? [];
  return Number(hours ?? 0) * 3600 + Number(minutes ?? 0) * 60 + Number(rest);
}

test('a click on a group opens its blocks, in the user time and without titles', async ({
  session,
}) => {
  const agent = newAgent();
  await withAgent(session, agent, async (page) => {
    await choose(page, 'On screen');
    await page
      .locator('#table-screen-by-app tbody tr', { hasText: 'Safari' })
      .locator('td')
      .nth(2)
      .click();
    await expect(page.getByRole('dialog', { name: 'Time on screen of Safari' })).toBeVisible();
    await expect(page.locator('#drill-count')).toHaveText('4 of 4 blocks');
    const rows = page.locator('#drill-blocks tbody tr');
    await expect(rows).toHaveCount(4);
    await expect(page.locator('#drill-blocks thead th')).toHaveText([
      'Start',
      'End',
      'Time',
      'App',
      'Website',
      'Project',
      'Ticket',
    ]);
    // Each column holds its own field: neighbours differ in all of them, the times are in the
    // zone of the window (nine o'clock UTC is four in New York) and the length is to the second.
    await expect(rows.nth(0).locator('td')).toHaveText([
      '2026-11-16 04:00',
      '2026-11-16 04:30',
      '30 min 7 s',
      'Safari',
      'example.org',
      'billing',
      'alpha-7',
    ]);
    await expect(rows.nth(1).locator('td')).toHaveText([
      '2026-11-16 05:00',
      '2026-11-16 05:29',
      '29 min 53 s',
      'Safari',
      'No site',
      'No project',
      'No ticket',
    ]);
    // The seconds of the blocks add up to the number of the row: 2 h.
    const lengths = await rows.locator('td:nth-child(3)').allTextContents();
    expect(lengths.reduce((sum, text) => sum + secondsOf(text), 0)).toBe(7200);
    await screenshotBothThemes(page, 'dashboard-screen-blocks');

    // No title is on the page, though the stand-in sent one.
    expect(await windowText(page)).not.toContain('secret window title');
    // The widget's own counting was asked for the group, not the sessions of any tool.
    expect(agent.blocks).toHaveLength(1);
    expect(agent.blocks[0]).toMatchObject({ widget: 'screen-by-app', group: 'Safari' });

    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog')).toHaveCount(0);

    // A bar opens the blocks of its day, counted as the time on screen with no sessions.
    await page
      .locator('#chart-screen-per-day')
      .getByRole('button', { name: 'Show the blocks of 2026-11-17' })
      .focus();
    await page.keyboard.press('Enter');
    await expect(page.getByRole('dialog', { name: 'Time on screen of 2026-11-17' })).toBeVisible();
    await expect(page.locator('#drill-count')).toHaveText('5 of 5 blocks');
    expect(agent.blocks[1]).toMatchObject({ source: 'on_screen', group_by: 'day' });
    expect(agent.blocks[1]).not.toHaveProperty('sessions');

    // The bar itself opens it too.
    await page.keyboard.press('Escape');
    await page.locator('#chart-screen-per-day .recharts-bar-rectangle').first().click();
    await expect(page.getByRole('dialog', { name: /Time on screen of 2026-11-/ })).toBeVisible();
  });
});

// Each grouping has its own table and its own drill-down: a failure of one shows on its own.
for (const [id, group, blocks] of [
  ['screen-by-app', 'Safari', 4],
  ['screen-by-site', 'example.org', 3],
  ['screen-by-project', 'billing', 6],
  ['screen-by-ticket', 'alpha-7', 5],
  ['screen-by-day', '2026-11-17', 5],
] as const) {
  test(`a click on a row of ${id} opens the blocks of ${group}`, async ({ session }) => {
    const agent = newAgent();
    await withAgent(session, agent, async (page) => {
      await choose(page, 'On screen');
      await page.locator(`#table-${id} tbody tr`).first().locator('td').nth(2).click();
      await expect(page.getByRole('dialog', { name: `Time on screen of ${group}` })).toBeVisible();
      await expect(page.locator('#drill-count')).toHaveText(`${blocks} of ${blocks} blocks`);
      await expect(page.locator('#drill-blocks tbody tr')).toHaveCount(blocks);
      // The row asks by its widget, which has the source and the grouping.
      expect(agent.blocks).toEqual([expect.objectContaining({ widget: id, group })]);
    });
  });
}

test('the blocks of a group come a page at a time, and add up to the row', async ({ session }) => {
  const agent = newAgent();
  await withAgent(session, agent, async (page) => {
    await choose(page, 'On screen');
    await page
      .locator('#table-screen-by-app tbody tr', { hasText: 'Terminal' })
      .locator('td')
      .nth(2)
      .click();
    await expect(page.locator('#drill-count')).toHaveText('20 of 25 blocks');
    await expect(page.locator('#drill-blocks tbody tr')).toHaveCount(20);
    await page.locator('#drill-more').click();
    await expect(page.locator('#drill-count')).toHaveText('25 of 25 blocks');
    await expect(page.locator('#drill-blocks tbody tr')).toHaveCount(25);
    await expect(page.locator('#drill-more')).toHaveCount(0);
    expect(agent.blocks.map((request) => request.offset)).toEqual([0, 20]);
    // The second page was added under the first, and the lengths of all of them are the row's.
    const lengths = await page.locator('#drill-blocks tbody tr td:nth-child(3)').allTextContents();
    expect(lengths.reduce((sum, text) => sum + secondsOf(text), 0)).toBe(1800);
    const starts = await page.locator('#drill-blocks tbody tr td:first-child').allTextContents();
    expect(new Set(starts).size).toBe(25);
  });
});

test('the page says when the collection is off, with a link to Settings', async ({ session }) => {
  await withAgent(session, newAgent({ screen: false, empty: true }), async (page) => {
    await choose(page, 'On screen');
    const off = page.locator('#dashboard-screen-off');
    await expect(off).toContainText('Apps and websites are not collected');
    await screenshotBothThemes(page, 'dashboard-screen-off');
    await off.getByRole('button', { name: 'Open Settings' }).click();
    await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible();
  });
});

test('the page keeps the numbers from before when the collection is turned off', async ({
  session,
}) => {
  await withAgent(session, newAgent({ screen: false }), async (page) => {
    await choose(page, 'On screen');
    await expect(page.locator('#dashboard-screen-off-note')).toContainText('not collected');
    await expect(page.locator('#value-screen-total')).toHaveText(formatMinutes(screenMinutes));
    await expect(page.locator('#dashboard-screen-off')).toHaveCount(0);
  });
});

test('the page says which permission is missing', async ({ session }) => {
  const grants = [
    { name: 'accessibility', state: 'granted' },
    { name: 'screen_recording', state: 'denied' },
    { name: 'automation', state: 'browser_not_running' },
    { name: 'input_monitoring', state: 'not_asked' },
  ] satisfies Agent['grants'];
  await withAgent(session, newAgent({ grants, empty: true }), async (page) => {
    await choose(page, 'On screen');
    const missing = page.locator('#dashboard-screen-permission');
    await expect(missing).toContainText('A permission is missing');
    await expect(missing).toContainText('Screen recording, Input monitoring');
    // A permission that is given, and a browser that is closed, are not named.
    await expect(missing).not.toContainText('Accessibility');
    await expect(missing).not.toContainText('Automation');
    await screenshotBothThemes(page, 'dashboard-screen-permission');
  });
  await withAgent(session, newAgent({ grants }), async (page) => {
    await choose(page, 'On screen');
    await expect(page.locator('#dashboard-screen-permission-note')).toContainText(
      'Screen recording, Input monitoring',
    );
    await expect(page.locator('#value-screen-total')).toHaveText(formatMinutes(screenMinutes));
  });
});

test('the page says when there is no time on screen in the period', async ({ session }) => {
  await withAgent(session, newAgent({ empty: true }), async (page) => {
    // The AI sessions have time, so the page opens on them.
    await expect(page.locator('#value-total')).toHaveText(formatMinutes(aiMinutes));
    await choose(page, 'On screen');
    await expect(page.locator('#dashboard-screen-empty')).toContainText(
      'No time on screen in this period',
    );
    await expect(page.locator('#value-screen-total')).toHaveCount(0);
    await screenshotBothThemes(page, 'dashboard-screen-empty');
  });
});

test('a computer that cannot collect the time on screen says so, whatever the stored switch is', async ({
  session,
}) => {
  // A settings file moved from another machine may hold `screen: true`; Settings has no switch here.
  const stored = { screen: true, supported: false };
  await withAgent(session, newAgent({ ...stored, empty: true }), async (page) => {
    await choose(page, 'On screen');
    await expect(page.locator('#dashboard-screen-off')).toContainText(
      'Apps and websites are not available here',
    );
    // There is no switch in Settings to send the person to, so there is no link to it.
    await expect(page.getByRole('button', { name: 'Open Settings' })).toHaveCount(0);
  });
  await withAgent(session, newAgent(stored), async (page) => {
    await choose(page, 'On screen');
    await expect(page.locator('#dashboard-screen-off-note')).toContainText('cannot collect');
    await expect(page.locator('#value-screen-total')).toHaveText(formatMinutes(screenMinutes));
    await expect(page.getByRole('button', { name: 'Open Settings' })).toHaveCount(0);
  });
});

test('the page says when it is reading, and has no switch of source until it has the numbers', async ({
  session,
}) => {
  const wait = gate();
  const agent = newAgent({ hold: wait.promise });
  await withAgent(session, agent, async (page) => {
    await expect(page.getByText('Reading the history')).toBeVisible();
    await expect(page.locator('#dashboard-source')).toHaveCount(0);
    await expect(page.locator('#value-total')).toHaveCount(0);
    wait.release();
    await expect(page.locator('#dashboard-source')).toBeVisible();
    await choose(page, 'On screen');
    await expect(page.locator('#value-screen-total')).toHaveText(formatMinutes(screenMinutes));
    await expect(page.getByText('Reading the history')).toHaveCount(0);
  });
});
