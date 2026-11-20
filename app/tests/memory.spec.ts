import type { Locator, Page } from '@playwright/test';
import { expect, test, windowText } from './fixtures';
import type { Session } from './fixtures';
import { newHost, openMemory, token, withHostAndAgent } from './memory-host';
import type { Fact, Host } from './memory-host';
import { screenshotBothThemes } from './screenshots';

/** What the stand-in agent answers: its settings, with collection as the host says. */
function agentAnswer(host: Host): (line: string) => string {
  return (line) =>
    line.startsWith('settings-get') && host.settingsFail
      ? JSON.stringify({ error: 'settings unavailable' })
      : line.startsWith('settings-get')
        ? JSON.stringify({
            model: { base_url: '', model: '', api_key_set: false },
            collection: { enabled: host.collecting, claude_code: true, codex: true, screen: false },
            supports: { screen: false },
          })
        : JSON.stringify({ status: 'running', version: '0.1.0', pid: 42 });
}

const inMemoryPage = (session: Session, host: Host, check: (page: Page) => Promise<void>) =>
  withHostAndAgent(session, host, agentAnswer(host), check);

const profile: Fact[] = [
  { entity_kind: 'identity', property: 'full_name', value: 'Ada Lovelace' },
  { entity_kind: 'role', property: 'job_title', value: 'Engineer' },
  { entity_kind: 'skill', property: 'skills', value: { name: 'Rust' } },
  { entity_kind: 'skill', property: 'skills', value: { name: 'Go' } },
  { entity_kind: 'preference', property: 'answer_language', value: 'English' },
  {
    entity_kind: 'preference',
    property: 'work_schedule',
    value: {
      days: 'Mon to Fri',
      hours: { start: '09:00', end: '17:00' },
      rest_days: ['Sat', 'Sun'],
    },
  },
];

test('the Memory page shows the facts as a tree with plain names and the time each was stored', async ({
  session,
}) => {
  const host = newHost(session);
  host.facts = profile;
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    const tree = page.locator('#memory-tree');
    for (const kind of ['Identity', 'Role', 'Skills', 'Preferences']) {
      await expect(tree.getByRole('heading', { name: kind, exact: true })).toBeVisible();
    }
    // A field with one fact: its plain name, its value and when it was stored.
    const fullName = tree.getByRole('listitem').filter({ hasText: 'Full name' });
    await expect(fullName).toContainText('Ada Lovelace');
    await expect(fullName).toContainText(/Stored 18 Nov 2026, \d\d:\d\d/);
    await expect(tree.getByText('Job title', { exact: true })).toBeVisible();
    await expect(tree.getByText('Answer language', { exact: true })).toBeVisible();
    // The list took three calls: two pages of two, and a page of two more.
    expect(host.urls.filter((url) => url.includes('limit=200'))).toHaveLength(3);
    await screenshotBothThemes(page, 'memory-tree');
  });
});

test('a list and a value with fields of its own start closed, and open and close', async ({
  session,
}) => {
  const host = newHost(session);
  host.facts = profile;
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    const tree = page.locator('#memory-tree');

    // The list of two skills.
    const skills = tree.getByRole('button', { name: 'Skills (2)' });
    await expect(skills).toHaveAttribute('aria-expanded', 'false');
    await expect(tree.getByText('Rust', { exact: true })).toHaveCount(0);
    await skills.click();
    await expect(skills).toHaveAttribute('aria-expanded', 'true');
    // Each skill is a value with fields of its own: closed again, named by its first text.
    const rust = tree.getByRole('button', { name: /^Rust/ });
    await expect(rust).toHaveAttribute('aria-expanded', 'false');
    await expect(tree.getByText('Name', { exact: true })).toHaveCount(0);
    await rust.click();
    await expect(rust).toHaveAttribute('aria-expanded', 'true');
    await expect(tree.getByText('Name', { exact: true })).toBeVisible();
    await rust.click();
    await expect(tree.getByText('Name', { exact: true })).toHaveCount(0);
    await skills.click();
    await expect(skills).toHaveAttribute('aria-expanded', 'false');
    await expect(tree.getByRole('button', { name: /^Rust/ })).toHaveCount(0);

    // One fact whose value has a subtree inside a subtree.
    const schedule = tree.getByRole('button', { name: /^Work schedule: Mon to Fri/ });
    await expect(schedule).toHaveAttribute('aria-expanded', 'false');
    await expect(tree.getByText('Days', { exact: true })).toHaveCount(0);
    await schedule.click();
    await expect(tree.getByText('Days', { exact: true })).toBeVisible();
    const hours = tree.getByRole('button', { name: 'Hours' });
    await expect(hours).toHaveAttribute('aria-expanded', 'false');
    await hours.click();
    await expect(tree.getByText('Start', { exact: true })).toBeVisible();
    await expect(tree.getByText('09:00', { exact: true })).toBeVisible();
    // An array inside the value is a list that starts closed too.
    const rest = tree.getByRole('button', { name: 'Rest days (2)' });
    await expect(rest).toHaveAttribute('aria-expanded', 'false');
    await expect(tree.getByText('Sat', { exact: true })).toHaveCount(0);
    await rest.click();
    await expect(rest).toHaveAttribute('aria-expanded', 'true');
    await expect(tree.getByText('Sat', { exact: true })).toBeVisible();
    await expect(tree.getByText('Sun', { exact: true })).toBeVisible();
    await screenshotBothThemes(page, 'memory-tree-open');
    await rest.click();
    await expect(tree.getByText('Sat', { exact: true })).toHaveCount(0);
    await schedule.click();
    await expect(tree.getByText('Days', { exact: true })).toHaveCount(0);
  });
});

/** The box of the first element the locator finds. */
async function boxOf(locator: Locator) {
  const box = await locator.first().boundingBox();
  expect(box).not.toBeNull();
  return box as NonNullable<typeof box>;
}

test('a short fact is one row: its name, its value and the time it was stored share a line', async ({
  session,
}) => {
  const host = newHost(session);
  host.facts = profile.slice(0, 2);
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    const row = page.locator('#memory-tree').getByRole('listitem').filter({ hasText: 'Full name' });
    const name = await boxOf(row.getByText('Full name', { exact: true }));
    const value = await boxOf(row.getByText('Ada Lovelace', { exact: true }));
    const stored = await boxOf(row.getByText(/^Stored /));
    // The name and the time start within a line of the value: none is on a line of its own.
    expect(Math.abs(value.y - name.y)).toBeLessThan(value.height);
    expect(Math.abs(stored.y - name.y)).toBeLessThan(value.height);
    // The time is at the end of the row, after the value.
    expect(stored.x).toBeGreaterThan(value.x + value.width);
    // The row is one line with a little padding, not three lines.
    const rowBox = await boxOf(row);
    expect(rowBox.height).toBeLessThan(value.height * 2.5);
  });
});

test('ten facts of one group take little room', async ({ session }) => {
  const host = newHost(session);
  host.facts = Array.from({ length: 10 }, (_, index) => ({
    entity_kind: 'preference',
    property: `property_${String(index + 1)}`,
    value: `Value ${String(index + 1)}`,
  }));
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    const tree = page.locator('#memory-tree');
    await expect(tree.getByText('Property 10', { exact: true })).toBeVisible();
    const first = await boxOf(tree.getByText('Property 1', { exact: true }));
    const last = await boxOf(tree.getByText('Property 10', { exact: true }));
    const windowHeight = await page.evaluate(() => window.innerHeight);
    // The ten rows fit in the part of the window under the heading, without scrolling.
    expect(last.y + last.height).toBeLessThan(windowHeight);
    expect((last.y - first.y) / 9).toBeLessThan(40);
    await screenshotBothThemes(page, 'memory-ten-facts');
  });
});

test('a long value wraps and shows whole', async ({ session }) => {
  const host = newHost(session);
  const long = Array.from({ length: 60 }, (_, index) => `word${String(index)}`).join(' ');
  host.facts = [
    { entity_kind: 'identity', property: 'full_name', value: 'Ada Lovelace' },
    { entity_kind: 'preference', property: 'bio', value: long },
  ];
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    const row = page.locator('#memory-tree').getByRole('listitem').filter({ hasText: 'Bio' });
    await expect(row.getByText(long, { exact: true })).toBeVisible();
    const value = await boxOf(row.getByText(long, { exact: true }));
    const short = await boxOf(
      page.locator('#memory-tree').getByText('Ada Lovelace', { exact: true }),
    );
    // More than one line, and the row grows to hold all of it: the value box is inside the row
    // box, so nothing the value needs is clipped by the row or by a parent.
    expect(value.height).toBeGreaterThan(short.height * 2);
    const rowBox = await boxOf(row);
    expect(value.y).toBeGreaterThanOrEqual(rowBox.y);
    expect(value.y + value.height).toBeLessThanOrEqual(rowBox.y + rowBox.height);
    // The time stays in the row, and the row does not reach past the window's width.
    const stored = await boxOf(row.getByText(/^Stored /));
    expect(stored.y + stored.height).toBeLessThanOrEqual(rowBox.y + rowBox.height);
    expect(rowBox.x + rowBox.width).toBeLessThanOrEqual(
      await page.evaluate(() => window.innerWidth),
    );
    await screenshotBothThemes(page, 'memory-long-value');
  });
});

test('a changed version brings a new fact without a reload', async ({ session }) => {
  test.setTimeout(90_000);
  const host = newHost(session);
  host.facts = profile.slice(0, 2);
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    const tree = page.locator('#memory-tree');
    await expect(tree.getByText('Job title', { exact: true })).toBeVisible();
    await expect(tree.getByText('Answer language', { exact: true })).toHaveCount(0);
    // The page asks for the version, one fact at a time, while nothing changes.
    await expect
      .poll(() => host.urls.filter((url) => url.includes('limit=1')).length)
      .toBeGreaterThan(0);
    expect(host.urls.filter((url) => url.includes('limit=200'))).toHaveLength(1);

    host.facts = [...host.facts, profile[4] as Fact];
    host.version = 2;
    await expect(tree.getByText('Answer language', { exact: true })).toBeVisible({
      timeout: 20_000,
    });
    await expect(tree.getByText('English', { exact: true })).toBeVisible();
  });
});

test('the collection line says whether collection is on and links to Settings', async ({
  session,
}) => {
  const host = newHost(session);
  host.facts = profile.slice(0, 1);
  host.collecting = false;
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    const line = page.locator('#memory-collection');
    await expect(line).toContainText('Collection is off');
    await line.getByRole('button', { name: 'Open Settings' }).click();
    await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Collection' })).toBeVisible();
  });
});

test('the page lists the facts and says collection is not known when the settings cannot be read', async ({
  session,
}) => {
  const host = newHost(session);
  host.facts = profile.slice(0, 2);
  host.settingsFail = true;
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    await expect(
      page.locator('#memory-tree').getByText('Full name', { exact: true }),
    ).toBeVisible();
    const line = page.locator('#memory-collection');
    await expect(line).toContainText('not known right now');
    await expect(line).toHaveAttribute('data-collecting', 'null');
    await expect(line.getByRole('button', { name: 'Open Settings' })).toBeVisible();
  });
});

test('an empty page says collection is not known when the settings cannot be read', async ({
  session,
}) => {
  const host = newHost(session);
  host.facts = [];
  host.version = null;
  host.settingsFail = true;
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    await expect(page.locator('#memory-empty')).toContainText('Nothing is learned yet');
    await expect(page.locator('#memory-empty')).toContainText('not known right now');
  });
});

for (const collecting of [true, false]) {
  test(`an empty page says that nothing is learned yet and that collection is ${collecting ? 'on' : 'off'}`, async ({
    session,
  }) => {
    const host = newHost(session);
    host.facts = [];
    host.version = null;
    host.collecting = collecting;
    await inMemoryPage(session, host, async (page) => {
      host.announce();
      await openMemory(page).click();
      const empty = page.locator('#memory-empty');
      const state = collecting ? 'on' : 'off';
      await expect(empty).toContainText('Nothing is learned yet');
      await expect(empty).toContainText(`Collection is ${state}`);
      await expect(page.locator('#memory-collection')).toContainText(`Collection is ${state}`);
      await screenshotBothThemes(page, `memory-empty-${state}`);
    });
  });
}

test('the page says in plain words that the agent is not running, then fills in when it is', async ({
  session,
}) => {
  test.setTimeout(90_000);
  const host = newHost(session);
  host.facts = profile.slice(0, 2);
  await inMemoryPage(session, host, async (page) => {
    // No connection file: nobody announced a host.
    await openMemory(page).click();
    const failed = page.locator('#memory-failed');
    await expect(failed).toContainText('The agent is not running');
    await screenshotBothThemes(page, 'memory-agent-absent');
    await expect(page.locator('#memory-tree')).toHaveCount(0);

    host.announce();
    await expect(page.locator('#memory-tree').getByText('Full name', { exact: true })).toBeVisible({
      timeout: 20_000,
    });
    await expect(failed).toHaveCount(0);
  });
});

test('the page says in plain words that Construct is not ready, then fills in when it is', async ({
  session,
}) => {
  test.setTimeout(90_000);
  const host = newHost(session);
  host.facts = profile.slice(0, 2);
  host.status = 503;
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    await expect(page.locator('#memory-failed')).toContainText('Construct is not ready');
    await screenshotBothThemes(page, 'memory-not-ready');

    host.status = 200;
    await expect(page.locator('#memory-tree').getByText('Full name', { exact: true })).toBeVisible({
      timeout: 20_000,
    });
  });
});

const deleteButton = (page: Page, name: string) =>
  page.getByRole('button', { name: `Delete ${name}`, exact: true });

test('Delete asks first, Cancel keeps the fact, and Yes removes the row and says what to do next', async ({
  session,
}) => {
  const host = newHost(session);
  host.facts = [...profile];
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    const tree = page.locator('#memory-tree');
    await deleteButton(page, 'Job title: Engineer').click();
    const dialog = page.getByRole('alertdialog', { name: 'Delete Job title: Engineer?' });
    await expect(dialog).toBeVisible();
    await screenshotBothThemes(page, 'memory-delete-dialog');

    await dialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(dialog).toHaveCount(0);
    await expect(tree.getByText('Engineer', { exact: true })).toBeVisible();
    expect(host.deletes).toEqual([]);

    await deleteButton(page, 'Job title: Engineer').click();
    await page.getByRole('button', { name: 'Yes, delete' }).click();
    await expect(tree.getByText('Engineer', { exact: true })).toHaveCount(0);
    await expect(tree.getByText('Job title', { exact: true })).toHaveCount(0);
    expect(host.deletes).toEqual(['fact-1']);
    await expect(page.locator('#memory-delete-done')).toContainText(
      'tell the chat the right value',
    );
    // The page reads the list again for the new version, and the fact stays gone.
    await expect
      .poll(() => host.urls.filter((url) => url.includes('limit=200')).length, { timeout: 20_000 })
      .toBeGreaterThan(3);
    await expect(tree.getByText('Engineer', { exact: true })).toHaveCount(0);
    await expect(tree.getByText('Ada Lovelace', { exact: true })).toBeVisible();
    await screenshotBothThemes(page, 'memory-after-delete');
  });
});

test('a value with fields of its own is deleted as one fact', async ({ session }) => {
  const host = newHost(session);
  host.facts = [...profile];
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    const tree = page.locator('#memory-tree');
    await deleteButton(page, 'Work schedule: Mon to Fri').click();
    await page.getByRole('button', { name: 'Yes, delete' }).click();
    await expect(tree.getByRole('button', { name: /^Work schedule/ })).toHaveCount(0);
    expect(host.deletes).toEqual(['fact-5']);
    // The other facts of the person stay.
    await expect(tree.getByText('Answer language', { exact: true })).toBeVisible();
  });
});

test('each fact of a list has its own Delete, and only that one goes', async ({ session }) => {
  const host = newHost(session);
  host.facts = [...profile];
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    const tree = page.locator('#memory-tree');
    await tree.getByRole('button', { name: 'Skills (2)' }).click();
    await deleteButton(page, 'Skills: Go').click();
    await page.getByRole('button', { name: 'Yes, delete' }).click();
    await expect(tree.getByRole('button', { name: /^Go/ })).toHaveCount(0);
    await expect(tree.getByRole('button', { name: /^Skills: Rust/ })).toBeVisible();
    expect(host.deletes).toEqual(['fact-3']);
  });
});

test('a fact the gear no longer has (404) counts as deleted', async ({ session }) => {
  const host = newHost(session);
  host.facts = [...profile.slice(0, 2)];
  host.deleteStatus = 404;
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    await deleteButton(page, 'Full name: Ada Lovelace').click();
    await page.getByRole('button', { name: 'Yes, delete' }).click();
    await expect(page.locator('#memory-tree').getByText('Ada Lovelace')).toHaveCount(0);
    await expect(page.locator('#memory-delete-done')).toBeVisible();
    expect(host.deletes).toEqual(['fact-0']);
  });
});

test('a failed delete keeps the row, says why, and sending it again works', async ({ session }) => {
  const host = newHost(session);
  host.facts = [...profile.slice(0, 2)];
  host.deleteStatus = 503;
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    const tree = page.locator('#memory-tree');
    await deleteButton(page, 'Full name: Ada Lovelace').click();
    await page.getByRole('button', { name: 'Yes, delete' }).click();
    await expect(page.locator('#memory-delete-failed')).toContainText(
      'Full name: Ada Lovelace is not deleted: Construct is not ready',
    );
    await expect(tree.getByText('Ada Lovelace', { exact: true })).toBeVisible();
    await expect(page.locator('#memory-delete-done')).toHaveCount(0);
    await screenshotBothThemes(page, 'memory-delete-failed');

    host.deleteStatus = 204;
    await deleteButton(page, 'Full name: Ada Lovelace').click();
    await page.getByRole('button', { name: 'Yes, delete' }).click();
    await expect(tree.getByText('Ada Lovelace')).toHaveCount(0);
    await expect(page.locator('#memory-delete-failed')).toHaveCount(0);
    expect(host.deletes).toEqual(['fact-0', 'fact-0']);
  });
});

test('Delete and its question work from the keyboard alone', async ({ session }) => {
  const host = newHost(session);
  host.facts = [...profile.slice(0, 2)];
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    const tree = page.locator('#memory-tree');
    const button = deleteButton(page, 'Full name: Ada Lovelace');
    await button.focus();
    await page.keyboard.press('Enter');
    const dialog = page.getByRole('alertdialog');
    await expect(dialog).toBeVisible();
    // Escape leaves the fact in place and gives the focus back to the button.
    await page.keyboard.press('Escape');
    await expect(dialog).toHaveCount(0);
    await expect(button).toBeFocused();
    expect(host.deletes).toEqual([]);

    await page.keyboard.press('Space');
    await expect(dialog).toBeVisible();
    // The focus lands on Cancel; Tab moves on to the button that deletes.
    await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeFocused();
    await page.keyboard.press('Tab');
    await expect(dialog.getByRole('button', { name: 'Yes, delete' })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(tree.getByText('Ada Lovelace')).toHaveCount(0);
    expect(host.deletes).toEqual(['fact-0']);
    // The button of the deleted fact is gone: the focus is on the line that says what was done.
    await expect(page.locator('#memory-delete-done')).toBeFocused();
  });
});

test('the row stays, and every Delete waits, until the gear has answered', async ({ session }) => {
  const host = newHost(session);
  host.facts = [...profile.slice(0, 2)];
  let release: () => void = () => undefined;
  host.deleteGate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    const tree = page.locator('#memory-tree');
    await deleteButton(page, 'Full name: Ada Lovelace').click();
    await page.getByRole('button', { name: 'Yes, delete' }).click();
    await expect.poll(() => host.deletes).toEqual(['fact-0']);
    // The host has the call and has not answered: the row is still there, and no other delete
    // can start.
    await expect(tree.getByText('Ada Lovelace', { exact: true })).toBeVisible();
    await expect(deleteButton(page, 'Job title: Engineer')).toBeDisabled();
    await expect(page.locator('#memory-delete-done')).toHaveCount(0);
    release();
    await expect(tree.getByText('Ada Lovelace')).toHaveCount(0);
    await expect(deleteButton(page, 'Job title: Engineer')).toBeEnabled();
  });
});

test('a list read that began before a delete does not bring the deleted row back', async ({
  session,
}) => {
  test.setTimeout(90_000);
  const host = newHost(session);
  host.facts = [...profile.slice(0, 2)];
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    const tree = page.locator('#memory-tree');
    await expect(tree.getByText('Ada Lovelace', { exact: true })).toBeVisible();

    // The version changes, so the page reads the list again; the host holds that read open.
    let release: () => void = () => undefined;
    host.listGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const readsBefore = host.urls.filter((url) => url.includes('limit=200')).length;
    host.version = 5;
    await expect
      .poll(() => host.urls.filter((url) => url.includes('limit=200')).length, {
        timeout: 20_000,
      })
      .toBeGreaterThan(readsBefore);

    // The fact is deleted while that read, which still lists it, is open.
    await deleteButton(page, 'Full name: Ada Lovelace').click();
    await page.getByRole('button', { name: 'Yes, delete' }).click();
    await expect(tree.getByText('Ada Lovelace')).toHaveCount(0);
    // Any read after this one is held too, so the page cannot put itself right before the check:
    // what the page shows is what the old read left, however long the check takes.
    const answered = host.listAnswered;
    host.listGate = new Promise<void>(() => undefined);
    release();
    await expect.poll(() => host.listAnswered).toBeGreaterThan(answered);
    // Time for the old list to reach the page. Without the guard its row is back and stays.
    await page.waitForTimeout(1500);
    expect(await tree.getByText('Ada Lovelace').count()).toBe(0);
    await expect(tree.getByText('Engineer', { exact: true })).toBeVisible();
  });
});

test('a fact the app cannot address has no Delete button', async ({ session }) => {
  const host = newHost(session);
  host.facts = [
    { id: '..', entity_kind: 'identity', property: 'full_name', value: 'Ada Lovelace' },
    { entity_kind: 'role', property: 'job_title', value: 'Engineer' },
  ];
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    const tree = page.locator('#memory-tree');
    await expect(tree.getByText('Ada Lovelace', { exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: /^Delete Full name/ })).toHaveCount(0);
    await expect(deleteButton(page, 'Job title: Engineer')).toBeVisible();
  });
});

test('the token and the port of the host never reach the window', async ({ session }) => {
  const host = newHost(session);
  host.facts = [...profile];
  await inMemoryPage(session, host, async (page) => {
    host.announce();
    await openMemory(page).click();
    await expect(page.locator('#memory-tree')).toBeVisible();
    const text = await windowText(page);
    expect(text).not.toContain(token);
    expect(text).not.toContain(`127.0.0.1:${host.port()}`);
    expect(text).not.toContain(`:${host.port()}`);
    // The bridge has no call that takes an address or a token.
    const names = await page.evaluate(() =>
      Object.keys(Reflect.get(window, 'constructAgent') as object),
    );
    expect(names.filter((name) => /memory/i.test(name)).sort()).toEqual([
      'deleteMemoryFact',
      'getMemoryFacts',
      'getMemoryVersion',
    ]);
  });
});
