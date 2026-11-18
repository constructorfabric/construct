import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import type { Page } from '@playwright/test';
import { isRecord } from '../src/agent-json';
import { claudePrompt, expect } from './fixtures';
import type { Session } from './fixtures';
import { ask, assistant, configure, test, toolLines } from './chat-helpers';
import type { Provider } from './provider';
import { screenshotBothThemes } from './screenshots';

// The page follows the file within 5 seconds of a change, the time the requirement gives. The
// trim step is the proof of that: the page is open when the file is written. After a chat turn the
// page is opened anew, so its bound only shows that it draws what the file holds.
const followMs = 5000;

interface Widget {
  id: string;
  kind: string;
  group_by?: string;
  position: { column: number; row: number; width: number; height: number };
}

interface Document {
  payload: { widgets: Widget[] };
}

function dashboardFile(session: Session): string {
  return path.join(session.workspaceDir, 'dashboards', 'activity.json');
}

function readDocument(session: Session): Document {
  const document: unknown = JSON.parse(readFileSync(dashboardFile(session), 'utf8'));
  if (!isRecord(document) || !isRecord(document.payload)) {
    throw new Error('the dashboard file has no payload');
  }
  return document as unknown as Document;
}

function widgetOf(document: Document, id: string): Widget {
  const found = document.payload.widgets.find((widget) => widget.id === id);
  if (found === undefined) {
    throw new Error(`the dashboard has no widget ${id}`);
  }
  return found;
}

/** The text of the whole file, as a model puts it in a `Write` call. */
const written = (document: Document): string => `${JSON.stringify(document, null, 2)}\n`;

async function openDashboard(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Dashboard', exact: true }).click();
}

async function openChat(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Chat', exact: true }).click();
}

/** The top edge of a widget on the page. */
async function top(page: Page, id: string): Promise<number> {
  const box = await page.locator(`#widget-${id}`).boundingBox();
  if (box === null) {
    throw new Error(`widget ${id} is not on the page`);
  }
  return box.y;
}

/** What the model got back from its last `count` calls, in order, as its newest request has them. */
function results(provider: Provider, count: number): string[] {
  const messages = provider.conversations[provider.conversations.length - 1];
  if (!Array.isArray(messages)) {
    throw new Error('the request carried no messages');
  }
  return messages
    .filter((message) => Object(message).role === 'tool')
    .map((message) => String(Object(message).content))
    .slice(-count);
}

/** Lets the model say `text` once its calls are done, and sends `message`. */
async function say(page: Page, provider: Provider, message: string, text: string): Promise<void> {
  provider.script = { chunks: [text], pauseMs: 0 };
  await ask(page, message);
  await expect(assistant(page).last()).toHaveText(text);
}

// One test, not five: the first start of the agent costs minutes, so the scenarios share one
// session, and each builds on the file the one before left.
test('a dashboard changed by chat shows on the page, and a wrong change is repaired', async ({
  session,
  page,
  provider,
}) => {
  // The first start of the agent unpacks its database, which is longer than the default timeout.
  test.setTimeout(240_000);
  claudePrompt(session, 'look at the week');
  // The agent reads the session file every few seconds, and the page asks for the numbers when it
  // opens: a page opened before the agent has read the prompt says there is nothing to show.
  await expect(async () => {
    await page.reload();
    await openDashboard(page);
    await expect(page.locator('#value-total')).toBeVisible({ timeout: 3000 });
  }).toPass({ timeout: 120_000 });

  // A dashboard the user has already trimmed: the file has no table by ticket.
  const trimmed = readDocument(session);
  trimmed.payload.widgets = trimmed.payload.widgets.filter((w) => w.id !== 'by-ticket');
  writeFileSync(dashboardFile(session), written(trimmed));
  await expect(page.locator('#widget-by-project')).toBeVisible({ timeout: followMs });
  await expect(page.locator('#widget-by-ticket')).toHaveCount(0, { timeout: followMs });
  await screenshotBothThemes(page, 'dashboard-chat-before');

  await openChat(page);
  await configure(page, { address: provider.url });

  // "add a table by ticket": the model edits the file, then looks at the result.
  const table = {
    id: 'by-ticket',
    kind: 'table',
    group_by: 'ticket',
    position: { column: 0, row: 14, width: 12, height: 5 },
  };
  provider.calls = [
    {
      name: 'Edit',
      arguments: {
        file_path: 'dashboards/activity.json',
        old_string: '"widgets": [',
        new_string: `"widgets": [\n${JSON.stringify(table)},`,
      },
    },
    { name: 'render_dashboard', arguments: {} },
  ];
  await say(page, provider, 'add a table by ticket', 'I added a table by ticket.');
  await expect(toolLines(page)).toHaveCount(2);
  await screenshotBothThemes(page, 'dashboard-chat-lines');
  expect(results(provider, 2)[1]).toContain('By ticket');
  await openDashboard(page);
  await expect(page.locator('#table-by-ticket')).toBeVisible({ timeout: followMs });
  await page.locator('#table-by-ticket').scrollIntoViewIfNeeded();
  await screenshotBothThemes(page, 'dashboard-chat-after');

  // "put the bars at the top": the bars take the first row and the total moves below them.
  await openChat(page);
  const moved = readDocument(session);
  widgetOf(moved, 'per-day').position = { column: 0, row: 0, width: 12, height: 4 };
  widgetOf(moved, 'total').position = { column: 0, row: 4, width: 12, height: 2 };
  provider.calls = [
    {
      name: 'Write',
      arguments: { file_path: 'dashboards/activity.json', content: written(moved) },
    },
    { name: 'render_dashboard', arguments: {} },
  ];
  await say(page, provider, 'put the bars at the top', 'The bars are at the top now.');
  await openDashboard(page);
  await expect(async () => {
    expect(await top(page, 'per-day')).toBeLessThan(await top(page, 'total'));
  }).toPass({ timeout: followMs });
  await expect(page.locator('#dashboard-problem')).toHaveCount(0);
  await screenshotBothThemes(page, 'dashboard-chat-bars-top');

  // A wrong edit: the file names a kind of widget that does not exist.
  await openChat(page);
  const wrong = readDocument(session);
  widgetOf(wrong, 'by-project').kind = 'pie';
  provider.calls = [
    {
      name: 'Write',
      arguments: { file_path: 'dashboards/activity.json', content: written(wrong) },
    },
    { name: 'render_dashboard', arguments: {} },
  ];
  await say(page, provider, 'make the project table a pie', 'The page did not take that.');
  // The model saw the errors and the note that the page keeps the last good version.
  expect(results(provider, 2)[1]).toContain('last good');
  await openDashboard(page);
  await expect(page.locator('#dashboard-problem')).toBeVisible({ timeout: followMs });
  // The page is still the last good dashboard: the table and the bars at the top are there.
  await expect(page.locator('#table-by-project')).toBeVisible();
  expect(await top(page, 'per-day')).toBeLessThan(await top(page, 'total'));
  await screenshotBothThemes(page, 'dashboard-chat-wrong-edit');

  // The model repairs it: it looks at the errors, writes the file right and looks again.
  await openChat(page);
  provider.calls = [
    { name: 'render_dashboard', arguments: {} },
    {
      name: 'Write',
      arguments: { file_path: 'dashboards/activity.json', content: written(moved) },
    },
    { name: 'render_dashboard', arguments: {} },
  ];
  await say(page, provider, 'fix it', 'I fixed the file.');
  expect(results(provider, 3)[0]).toContain('last good');
  expect(results(provider, 3)[2]).not.toContain('last good');
  await openDashboard(page);
  await expect(page.locator('#dashboard-problem')).toHaveCount(0, { timeout: followMs });
  await expect(page.locator('#table-by-project')).toBeVisible();
  await screenshotBothThemes(page, 'dashboard-chat-repaired');

  // "Where did my time go this week?": the model's numbers are the page's.
  const total = await page.locator('#value-total').innerText();
  await openChat(page);
  provider.calls = [{ name: 'render_dashboard', arguments: {} }];
  await say(page, provider, 'Where did my time go this week?', `This week you worked ${total}.`);
  expect(results(provider, 1)[0]).toContain(`Total: ${total}`);
});
