import { appendFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import type { Page } from '@playwright/test';
import type { ElectronApplication } from '@playwright/test';
import { firstCells, now } from './dashboard-shared';
import { expect, expectNoPolicyViolations, openApp, test as base, windowText } from './fixtures';
import type { Session } from './fixtures';
import { screenshotBothThemes } from './screenshots';

/** Sessions of each project on Tuesday 17 November: more than a page for the first project. */
const sessionsOf = { billing: 25, search: 2 } as const;

/** Writes a Claude Code session file for each session: two prompts a minute apart, every 10 minutes. */
function seedSessions(session: Session): void {
  let hour = 8;
  for (const [project, count] of Object.entries(sessionsOf)) {
    for (let index = 0; index < count; index += 1) {
      const file = path.join(
        session.claudeHome,
        'projects',
        `-w-${project}`,
        `${project}-${String(index).padStart(2, '0')}.jsonl`,
      );
      mkdirSync(path.dirname(file), { recursive: true });
      for (let prompt = 0; prompt < 2; prompt += 1) {
        const at = new Date(Date.UTC(2026, 10, 17, hour, index * 10 + prompt));
        appendFileSync(
          file,
          `${JSON.stringify({
            type: 'user',
            timestamp: at.toISOString(),
            cwd: `/w/${project}`,
            sessionId: `${String(index).padStart(2, '0')}-${project}-0000`,
            message: { content: `secret words for ${project} number ${index}` },
          })}\n`,
        );
      }
    }
    hour += 6;
  }
}

/**
 * The window runs in a zone that is not UTC, so a time that is shown unconverted, or converted the
 * wrong way, does not match the literals below.
 */
const zone = 'America/New_York';

const test = base.extend<{ dashboard: { app: ElectronApplication; page: Page } }>({
  dashboard: async ({ session }, use) => {
    seedSessions(session);
    const { app, page } = await openApp({ ...session, env: { ...session.env, TZ: zone } });
    await page.clock.install({ time: now });
    // The page counts when it opens, and the agent reads the session files every few seconds.
    await expect(async () => {
      await page.reload();
      await page.getByRole('button', { name: 'Dashboard', exact: true }).click();
      await expect(page.locator('#table-by-project tbody tr')).toHaveCount(2, { timeout: 2_000 });
    }).toPass({ timeout: 60_000 });
    await app.evaluate(({ BrowserWindow }) => {
      BrowserWindow.getAllWindows()[0]?.setSize(1400, 1000);
    });
    try {
      await use({ app, page });
      await expectNoPolicyViolations(page);
    } finally {
      await app.close();
    }
  },
});

const rows = (page: Page): ReturnType<Page['locator']> => page.locator('#drill-sessions tbody tr');

/** The minutes of a time as the page writes it: `1 h 5 min`, `45 min`. */
function minutesOf(text: string): number {
  const [, hours, minutes] = /^(?:(\d+) h)?\s*(?:(\d+) min)?$/.exec(text.trim()) ?? [];
  return Number(hours ?? 0) * 60 + Number(minutes ?? 0);
}

test('a click on a project row opens its sessions, which add up to the row, a page at a time', async ({
  dashboard: { page },
}) => {
  await expect.poll(() => firstCells(page, 'by-project')).toEqual(['billing', 'search']);
  const row = page.locator('#table-by-project tbody tr', { hasText: 'billing' });
  const rowTime = minutesOf(await row.locator('td').nth(1).innerText());
  await expect(row.locator('td').nth(3)).toHaveText('25');

  await row.locator('td').nth(2).click();
  await expect(page.getByRole('dialog', { name: 'Sessions of billing' })).toBeVisible();
  await expect(page.locator('#drill-count')).toHaveText('20 of 25 sessions');
  await expect(rows(page)).toHaveCount(20);
  await expect(page.locator('#drill-more')).toBeVisible();

  // Start and end are in the user's time, with the length, the tool, the project and the ticket.
  const first = rows(page).first().locator('td');
  await expect(first.nth(0)).toHaveText('2026-11-17 03:00');
  await expect(first.nth(1)).toHaveText('2026-11-17 03:02');
  await expect(first.nth(2)).toHaveText('2 min');
  await expect(first.nth(3)).toHaveText('claude-code');
  await expect(first.nth(4)).toHaveText('billing');
  await expect(first.nth(5)).toHaveText('No ticket');
  await expect(first.nth(6)).toHaveText('00-billi');
  await screenshotBothThemes(page, 'dashboard-drilldown');

  // The next page comes under the first, and they are all the row's sessions.
  await page.locator('#drill-more').click();
  await expect(page.locator('#drill-count')).toHaveText('25 of 25 sessions');
  await expect(rows(page)).toHaveCount(25);
  await expect(page.locator('#drill-more')).toHaveCount(0);
  // The seed has no two sessions at the same minute, so the lengths add up too; a minute that two
  // parallel sessions share counts whole in each, and then they would add up to more.
  const lengths = await rows(page).locator('td:nth-child(3)').allTextContents();
  expect(lengths.reduce((sum, text) => sum + minutesOf(text), 0)).toBe(rowTime);
  expect(new Set(await rows(page).locator('td:nth-child(7)').allTextContents()).size).toBe(25);
  expect(await rows(page).locator('td:nth-child(5)').allTextContents()).toEqual(
    Array<string>(25).fill('billing'),
  );

  // Nothing the user said is on the page.
  expect(await windowText(page)).not.toContain('secret words');

  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
});

test('a click on a bar opens the sessions of its day', async ({ dashboard: { page } }) => {
  // The chart's table is for the keyboard and the screen reader: it opens the day too.
  await page.getByRole('button', { name: 'Show the sessions of 2026-11-17' }).focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('dialog', { name: 'Sessions of 2026-11-17' })).toBeVisible();
  const total = Object.values(sessionsOf).reduce((sum, count) => sum + count, 0);
  await expect(page.locator('#drill-count')).toHaveText(`20 of ${total} sessions`);
  await page.locator('#drill-more').click();
  await expect(page.locator('#drill-count')).toHaveText(`${total} of ${total} sessions`);
  await expect(rows(page)).toHaveCount(total);
  // The sessions add up to the bar of the day, which the chart's table also gives in minutes.
  const bar = page.locator('#chart-per-day table tbody tr', { hasText: '2026-11-17' });
  const lengths = await rows(page).locator('td:nth-child(3)').allTextContents();
  expect(lengths.reduce((sum, text) => sum + minutesOf(text), 0)).toBe(
    minutesOf(await bar.locator('td').first().innerText()),
  );
  await page.keyboard.press('Escape');

  // The bar itself opens it too.
  await page.locator('#chart-per-day .recharts-bar-rectangle').first().click();
  await expect(page.getByRole('dialog', { name: 'Sessions of 2026-11-17' })).toBeVisible();
});

test('the end-of-day summary has a line for the day, and Copy puts the lines on the clipboard', async ({
  dashboard: { app, page },
}) => {
  await page.locator('#dashboard-previous-day').click();
  const summary = page.locator('#summary-end-of-day');
  await expect(summary.locator('li')).toHaveText([
    'No ticket (billing): 50 min',
    'No ticket (search): 4 min',
  ]);
  await summary.scrollIntoViewIfNeeded();
  await screenshotBothThemes(page, 'dashboard-summary');

  await page.locator('#summary-end-of-day-copy').click();
  await expect(summary.getByRole('status')).toHaveText('Copied');
  expect(await app.evaluate(({ clipboard }) => clipboard.readText())).toBe(
    'No ticket (billing): 50 min\nNo ticket (search): 4 min',
  );
});
