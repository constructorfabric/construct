import { appendFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import type { Page } from '@playwright/test';
import { firstCells, now, settingsReply } from './dashboard-shared';
import {
  expect,
  expectNoPolicyViolations,
  gate,
  openApp,
  restartAgent,
  test as base,
  withStandIn,
} from './fixtures';
import type { Session } from './fixtures';
import { screenshotBothThemes } from './screenshots';

/** The minutes the user worked on each project on Tuesday 17 November, one prompt a minute. */
const worked = { billing: 20, search: 40, docs: 10 } as const;

/** Writes a Claude Code session file for each project, as the tool does while it works. */
function seedSessions(session: Session): void {
  let hour = 9;
  for (const [project, minutes] of Object.entries(worked)) {
    const file = path.join(session.claudeHome, 'projects', `-w-${project}`, `${project}.jsonl`);
    mkdirSync(path.dirname(file), { recursive: true });
    for (let minute = 0; minute < minutes; minute += 1) {
      const at = new Date(Date.UTC(2026, 10, 17, hour, minute));
      appendFileSync(
        file,
        `${JSON.stringify({
          type: 'user',
          timestamp: at.toISOString(),
          cwd: `/w/${project}`,
          sessionId: project,
          message: { content: `work on ${project}, step ${minute}` },
        })}\n`,
      );
    }
    hour += 2;
  }
}

const rowNames = (page: Page): Promise<string[]> => firstCells(page, 'by-project');

/** Opens the Dashboard page, and waits until the agent has read all the sessions. */
async function openDashboard(
  session: Session,
  rows = 3,
  total = '1 h 10 min',
): Promise<{ page: Page; close: () => Promise<void> }> {
  const { app, page } = await openApp(session);
  await page.clock.install({ time: now });
  // The page counts when it opens, and the agent reads the session files every few seconds.
  await expect(async () => {
    await page.reload();
    await page.getByRole('button', { name: 'Dashboard', exact: true }).click();
    await expect(page.locator('#table-by-project tbody tr')).toHaveCount(rows, {
      timeout: 2_000,
    });
    await expect(page.locator('#value-total')).toHaveText(total, { timeout: 2_000 });
  }).toPass({ timeout: 60_000 });
  // A window the size of a desk screen shows the tables, and so the screenshots do.
  await app.evaluate(({ BrowserWindow }) => {
    BrowserWindow.getAllWindows()[0]?.setSize(1400, 1000);
  });
  return {
    page,
    close: async () => {
      await expectNoPolicyViolations(page);
      await app.close();
    },
  };
}

/** The Dashboard page of an agent that has read the three projects, and that is closed after. */
const test = base.extend<{ dashboard: Page }>({
  dashboard: async ({ session }, use) => {
    seedSessions(session);
    const opened = await openDashboard(session);
    try {
      await use(opened.page);
    } finally {
      await opened.close();
    }
  },
});

/** Screenshots of the page with the table of the projects in view. */
async function screenshotProjects(page: Page, name: string): Promise<void> {
  await page
    .getByRole('heading', { name: 'Time by project' })
    .evaluate((heading) => heading.scrollIntoView({ block: 'center' }));
  await screenshotBothThemes(page, name);
}

/** Opens the menu of a row with the keyboard, and chooses an item with Home or End and Enter. */
async function chooseWithKeyboard(
  page: Page,
  project: string,
  item: 'Hide' | 'Rename',
): Promise<void> {
  const trigger = page.getByRole('button', { name: `Actions for ${project}`, exact: true });
  await trigger.focus();
  await page.keyboard.press('Enter');
  // The menu takes the focus a moment after it opens; a key pressed before is pressed on the button.
  await expect(page.locator('[role="menu"]:focus-within')).toHaveCount(1);
  // Home and End go to the first and the last item whatever the menu has highlighted so far (the
  // pointer may rest on an item, which highlights it too).
  await expect(async () => {
    await page.keyboard.press(item === 'Hide' ? 'Home' : 'End');
    await expect(page.getByRole('menuitem', { name: item })).toHaveAttribute('data-highlighted', {
      timeout: 500,
    });
  }).toPass();
  await page.keyboard.press('Enter');
}

async function renameWithKeyboard(page: Page, project: string, to: string): Promise<void> {
  await chooseWithKeyboard(page, project, 'Rename');
  const field = page.getByRole('textbox', { name: 'Name to show' });
  await expect(field).toBeFocused();
  await page.keyboard.press('ControlOrMeta+A');
  await page.keyboard.type(to);
  await page.keyboard.press('Enter');
  await expect(field).toHaveCount(0);
}

const focusedProject = (page: Page): ReturnType<Page['locator']> =>
  page.locator('#table-by-project [data-project]:focus');

test('hiding a project takes it out of the total, and showing the hidden ones brings it back', async ({
  dashboard: page,
}) => {
  await expect.poll(() => rowNames(page)).toEqual(['search', 'billing', 'docs']);
  await expect(page.locator('#dashboard-hidden-note')).toHaveCount(0);
  // Opening the page does not move the focus into the table.
  await expect(focusedProject(page)).toHaveCount(0);
  await screenshotProjects(page, 'dashboard-projects');

  // The row's button is on the way of the Tab key, after the controls above the page.
  await page.locator('#dashboard-next-week').focus();
  await expect(async () => {
    await page.keyboard.press('Tab');
    await expect(focusedProject(page)).toHaveCount(1, { timeout: 200 });
  }).toPass();
  await expect(focusedProject(page)).toHaveAccessibleName('Actions for search');

  // Hide works by the keyboard alone, and the focus stays in the table.
  await chooseWithKeyboard(page, 'search', 'Hide');
  await expect.poll(() => rowNames(page)).toEqual(['billing', 'docs']);
  await expect(page.locator('#value-total')).toHaveText('30 min');
  await expect(page.locator('#dashboard-hidden-note')).toContainText('1 project is hidden.');
  await expect(focusedProject(page)).toHaveCount(1);
  await screenshotProjects(page, 'dashboard-projects-hidden');

  await page.getByRole('button', { name: 'Show hidden projects' }).click();
  await expect.poll(() => rowNames(page)).toEqual(['search', 'billing', 'docs']);
  await expect(page.locator('#value-total')).toHaveText('1 h 10 min');
  await expect(page.locator('#dashboard-hidden-note')).toHaveCount(0);
});

test('with every project hidden the page still says so and shows them again', async ({
  dashboard: page,
}) => {
  await expect.poll(() => rowNames(page)).toEqual(['search', 'billing', 'docs']);
  await chooseWithKeyboard(page, 'search', 'Hide');
  await expect.poll(() => rowNames(page)).toEqual(['billing', 'docs']);
  await chooseWithKeyboard(page, 'billing', 'Hide');
  await expect.poll(() => rowNames(page)).toEqual(['docs']);
  await chooseWithKeyboard(page, 'docs', 'Hide');

  await expect(page.locator('#dashboard-all-hidden')).toContainText('Every project is hidden');
  // The row button that had the focus is gone: the focus is on the Show button.
  await expect(page.getByRole('button', { name: 'Show hidden projects' })).toBeFocused();
  await expect(page.locator('#dashboard-hidden-note')).toContainText('3 projects are hidden.');

  // One press brings back all three, not only the first.
  await page.keyboard.press('Enter');
  await expect.poll(() => rowNames(page)).toEqual(['search', 'billing', 'docs']);
  await expect(page.locator('#value-total')).toHaveText('1 h 10 min');
  await expect(page.locator('#dashboard-hidden-note')).toHaveCount(0);
  // The Show button is gone too: the focus is on the first row's button.
  await expect(focusedProject(page)).toHaveAccessibleName('Actions for search');
});

test('renaming a project to the name of another makes one row of the two', async ({
  dashboard: page,
}) => {
  await expect.poll(() => rowNames(page)).toEqual(['search', 'billing', 'docs']);
  await renameWithKeyboard(page, 'docs', 'search');
  await expect.poll(() => rowNames(page)).toEqual(['search', 'billing']);
  const merged = page.locator('#table-by-project tbody tr').first();
  await expect(merged.locator('td').nth(1)).toHaveText('50 min');
  await expect(page.locator('#value-total')).toHaveText('1 h 10 min');
  // The focus goes to the row the project is in now.
  await expect(focusedProject(page)).toHaveAccessibleName('Actions for search');

  // A row that was renamed is renamed again by its name on the page.
  await renameWithKeyboard(page, 'search', 'Search and docs');
  await expect.poll(() => rowNames(page)).toEqual(['Search and docs', 'billing']);
  await expect(page.locator('#table-by-project tbody tr').first().locator('td').nth(1)).toHaveText(
    '50 min',
  );
  await screenshotProjects(page, 'dashboard-projects-renamed');
});

test('the rename dialog starts from the name of the row, and an empty name is not saved', async ({
  dashboard: page,
}) => {
  await expect.poll(() => rowNames(page)).toEqual(['search', 'billing', 'docs']);
  const field = page.getByRole('textbox', { name: 'Name to show' });
  await chooseWithKeyboard(page, 'billing', 'Rename');
  await expect(field).toHaveValue('billing');
  await expect(field).toBeFocused();
  await page.keyboard.type(' abandoned');
  await expect(field).toHaveValue('billing abandoned');
  await page.keyboard.press('Escape');
  await expect(field).toHaveCount(0);

  await chooseWithKeyboard(page, 'billing', 'Rename');
  await expect(field).toHaveValue('billing');
  await expect(field).toBeFocused();
  // Enter on an empty name saves nothing and leaves the dialog open.
  await page.keyboard.press('ControlOrMeta+A');
  await page.keyboard.press('Backspace');
  await page.keyboard.press('Enter');
  await expect(field).toBeVisible();
  await expect(page.getByRole('button', { name: 'Rename', exact: true })).toBeDisabled();
  await page.keyboard.press('Escape');
  await expect.poll(() => rowNames(page)).toEqual(['search', 'billing', 'docs']);
});

test('the choices are kept after a restart of the app and of the agent', async ({ session }) => {
  seedSessions(session);
  const first = await openDashboard(session);
  try {
    await expect.poll(() => rowNames(first.page)).toEqual(['search', 'billing', 'docs']);
    await chooseWithKeyboard(first.page, 'billing', 'Hide');
    await expect.poll(() => rowNames(first.page)).toEqual(['search', 'docs']);
    await renameWithKeyboard(first.page, 'docs', 'Documentation');
    await expect.poll(() => rowNames(first.page)).toEqual(['search', 'Documentation']);
  } finally {
    await first.close();
  }

  await restartAgent(session);
  const second = await openDashboard(session, 2, '50 min');
  try {
    await expect.poll(() => rowNames(second.page)).toEqual(['search', 'Documentation']);
    await expect(second.page.locator('#dashboard-hidden-note')).toContainText(
      '1 project is hidden.',
    );
  } finally {
    await second.close();
  }
});

/** An agent that has two projects and answers each change as the real one does. */
function projectsAgent(): {
  readonly lines: string[];
  hidden: string[];
  refuse: boolean;
  /** The agent cannot give its settings: the page fails. */
  settingsError: boolean;
  /** Held replies: the lookups at the document, the numbers and the first change. */
  gate: {
    document: Promise<void> | null;
    numbers: Promise<void> | null;
    hide: Promise<void> | null;
  };
  lookups: number;
  /** How many times the numbers were asked for, counted when the request arrives. */
  numberCalls: number;
  answer: (line: string) => Promise<string>;
} {
  const projects = [
    { key: 'billing', minutes: 30 },
    { key: 'search', minutes: 60 },
  ];
  const document = (hidden: readonly string[]): unknown => ({
    type: 'a dashboard type',
    title: 'Activity',
    payload: {
      period: 'this_week',
      sessions: 'own',
      widgets: [
        {
          id: 'by-project',
          kind: 'table',
          group_by: 'project',
          position: { column: 0, row: 0, width: 12, height: 5 },
        },
      ],
      hidden_projects: hidden,
      project_names: {},
    },
  });
  const agent = {
    lines: [] as string[],
    hidden: [] as string[],
    refuse: false,
    settingsError: false,
    gate: { document: null, numbers: null, hide: null } as {
      document: Promise<void> | null;
      numbers: Promise<void> | null;
      hide: Promise<void> | null;
    },
    lookups: 0,
    numberCalls: 0,
    answer: async (line: string): Promise<string> => {
      agent.lines.push(line);
      if (line.startsWith('dashboard-numbers ')) {
        agent.numberCalls += 1;
        const seen = agent.hidden;
        await (agent.gate.numbers ?? Promise.resolve());
        const shown = projects.filter((project) => !seen.includes(project.key));
        const total = shown.reduce((sum, project) => sum + project.minutes, 0);
        return JSON.stringify({
          total_minutes: total,
          session_count: shown.length,
          hidden_project_count: seen.length,
          days: [{ label: '2026-11-16', minutes: total }],
          groups: shown.map((project) => ({
            key: project.key,
            minutes: project.minutes,
            share: project.minutes / total,
            session_count: 1,
          })),
        });
      }
      if (line === 'dashboard-get') {
        // The look is answered with the document as it was when it began.
        const snapshot = [...agent.hidden];
        agent.lookups += 1;
        await (agent.gate.document ?? Promise.resolve());
        return JSON.stringify({ document: document(snapshot) });
      }
      if (line.startsWith('dashboard-hide ') || line.startsWith('dashboard-show ')) {
        const { project } = JSON.parse(line.slice(line.indexOf(' ') + 1)) as { project: string };
        const gate = agent.gate.hide;
        agent.gate.hide = null;
        await (gate ?? Promise.resolve());
        if (agent.refuse) {
          return JSON.stringify({ error: 'the dashboard file could not be written' });
        }
        agent.hidden = line.startsWith('dashboard-hide ')
          ? [...agent.hidden, project]
          : agent.hidden.filter((name) => name !== project);
        return JSON.stringify({ document: document(agent.hidden) });
      }
      if (line === 'settings-get' && agent.settingsError) {
        return JSON.stringify({ error: 'settings unavailable' });
      }
      return settingsReply(true);
    },
  };
  return agent;
}

async function openStandIn(
  session: Session,
  agent: ReturnType<typeof projectsAgent>,
  check: (page: Page) => Promise<void>,
): Promise<void> {
  await withStandIn(session, agent.answer, async (page) => {
    await page.clock.install({ time: now });
    await page.reload();
    await page.getByRole('button', { name: 'Dashboard', exact: true }).click();
    await expect.poll(() => rowNames(page)).toEqual(['search', 'billing']);
    await check(page);
  });
}

test('a change the agent refuses is said on the page, and can be dismissed', async ({
  session,
}) => {
  const agent = projectsAgent();
  agent.refuse = true;
  await openStandIn(session, agent, async (page) => {
    await chooseWithKeyboard(page, 'billing', 'Hide');
    const failed = page.locator('#dashboard-change-failed');
    await expect(failed).toContainText(
      'The change was not made: the dashboard file could not be written',
    );
    expect(await rowNames(page)).toEqual(['search', 'billing']);
    expect(agent.lines).toContain('dashboard-hide {"project":"billing"}');
    await page.getByRole('button', { name: 'Dismiss' }).click();
    await expect(failed).toHaveCount(0);
  });
});

/** After a stale answer has arrived the page still holds the document the change made. */
async function expectNewDocumentKept(
  page: Page,
  agent: ReturnType<typeof projectsAgent>,
): Promise<void> {
  await page.waitForTimeout(500);
  expect(await rowNames(page)).toEqual(['search']);
  await expect(page.locator('#dashboard-hidden-note')).toContainText('1 project is hidden.');
  // Show sends the hidden project, which the old document lacks.
  await page.getByRole('button', { name: 'Show hidden projects' }).click();
  await expect
    .poll(() => agent.lines.filter((line) => line.startsWith('dashboard-show')))
    .toEqual(['dashboard-show {"project":"billing"}']);
}

test('a refused Show leaves no focus waiting for the rows that come back later', async ({
  session,
}) => {
  const agent = projectsAgent();
  await openStandIn(session, agent, async (page) => {
    await chooseWithKeyboard(page, 'billing', 'Hide');
    await expect.poll(() => rowNames(page)).toEqual(['search']);
    await chooseWithKeyboard(page, 'search', 'Hide');
    const show = page.getByRole('button', { name: 'Show hidden projects' });
    await expect(show).toBeFocused();

    agent.refuse = true;
    await page.keyboard.press('Enter');
    await expect(page.locator('#dashboard-change-failed')).toBeVisible();

    // The file is edited outside the app, and the rows come back on the next look.
    agent.refuse = false;
    agent.hidden = [];
    await expect.poll(() => rowNames(page)).toEqual(['search', 'billing']);
    await expect(focusedProject(page)).toHaveCount(0);
  });
});

test('a look at the document that began before a change does not bring the old document back', async ({
  session,
}) => {
  const agent = projectsAgent();
  await openStandIn(session, agent, async (page) => {
    let release = (): void => undefined;
    agent.gate.document = new Promise<void>((resolve) => {
      release = resolve;
    });
    // The page asks again every two seconds: the next look is held, with the old document.
    const before = agent.lookups;
    await expect.poll(() => agent.lookups).toBeGreaterThan(before);
    await chooseWithKeyboard(page, 'billing', 'Hide');
    await expect.poll(() => rowNames(page)).toEqual(['search']);
    release();
    agent.gate.document = null;
    await expectNewDocumentKept(page, agent);
  });
});

test('a look that begins during a change and is answered after it does not bring the old document back', async ({
  session,
}) => {
  const agent = projectsAgent();
  await openStandIn(session, agent, async (page) => {
    let releaseChange = (): void => undefined;
    agent.gate.hide = new Promise<void>((resolve) => {
      releaseChange = resolve;
    });
    await chooseWithKeyboard(page, 'billing', 'Hide');
    await expect
      .poll(() => agent.lines.filter((line) => line.startsWith('dashboard-hide')))
      .toHaveLength(1);
    // A look begins while the change is waiting for its answer, and is held with the old document.
    let releaseLook = (): void => undefined;
    agent.gate.document = new Promise<void>((resolve) => {
      releaseLook = resolve;
    });
    const before = agent.lookups;
    await expect.poll(() => agent.lookups).toBeGreaterThan(before);
    releaseChange();
    await expect.poll(() => rowNames(page)).toEqual(['search']);
    releaseLook();
    agent.gate.document = null;
    await expectNewDocumentKept(page, agent);
  });
});

test('the rows stay while the numbers are counted again after a change', async ({ session }) => {
  const agent = projectsAgent();
  await openStandIn(session, agent, async (page) => {
    let release = (): void => undefined;
    agent.gate.numbers = new Promise<void>((resolve) => {
      release = resolve;
    });
    const before = agent.numberCalls;
    await chooseWithKeyboard(page, 'billing', 'Hide');
    // The page took the new document when it asked for the numbers again, and the reply is held.
    await expect.poll(() => agent.numberCalls).toBeGreaterThan(before);
    // The old numbers are on show, and the page does not turn to its reading state.
    await expect(page.locator('#dashboard-reading')).toHaveCount(0);
    expect(await rowNames(page)).toEqual(['search', 'billing']);
    release();
    agent.gate.numbers = null;
    await expect.poll(() => rowNames(page)).toEqual(['search']);
    await expect(page.locator('#dashboard-hidden-note')).toBeVisible();
  });
});

test('a page that was shown gives way to the loader at once when it is counted for another week', async ({
  session,
}) => {
  const agent = projectsAgent();
  await openStandIn(session, agent, async (page) => {
    const numbers = gate();
    agent.gate.numbers = numbers.promise;
    await page.locator('#dashboard-previous-week').click();
    // No retrying: the loader is there at the moment of the click, not after a quarter second.
    expect(await page.locator('#dashboard-reading').count()).toBe(1);
    numbers.release();
    agent.gate.numbers = null;
    await expect(page.locator('#dashboard-reading')).toHaveCount(0);
  });
});

test('a page that failed gives way to the loader at once when it is counted for another week', async ({
  session,
}) => {
  const agent = projectsAgent();
  agent.settingsError = true;
  await withStandIn(session, agent.answer, async (page) => {
    await page.clock.install({ time: now });
    await page.reload();
    await page.getByRole('button', { name: 'Dashboard', exact: true }).click();
    await expect(page.locator('#dashboard-failed')).toBeVisible();
    agent.gate.numbers = new Promise<void>(() => undefined);
    await page.locator('#dashboard-previous-week').click();
    // No retrying: the page has answered once, so the loader is there with no wait.
    expect(await page.locator('#dashboard-reading').count()).toBe(1);
  });
});

test('two changes are sent one after the other, in the order they were made', async ({
  session,
}) => {
  const agent = projectsAgent();
  await openStandIn(session, agent, async (page) => {
    let release = (): void => undefined;
    agent.gate.hide = new Promise<void>((resolve) => {
      release = resolve;
    });
    await chooseWithKeyboard(page, 'billing', 'Hide');
    await expect
      .poll(() => agent.lines.filter((line) => line.startsWith('dashboard-hide')))
      .toHaveLength(1);
    await chooseWithKeyboard(page, 'search', 'Hide');
    await page.waitForTimeout(500);
    // The second change waits for the answer to the first.
    expect(agent.lines.filter((line) => line.startsWith('dashboard-hide'))).toEqual([
      'dashboard-hide {"project":"billing"}',
    ]);
    release();
    await expect
      .poll(() => agent.lines.filter((line) => line.startsWith('dashboard-hide')))
      .toEqual(['dashboard-hide {"project":"billing"}', 'dashboard-hide {"project":"search"}']);
    await expect(page.locator('#dashboard-all-hidden')).toBeVisible();
  });
});
