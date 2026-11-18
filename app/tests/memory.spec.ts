import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import type { Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import path from 'node:path';
import type { Page } from '@playwright/test';
import { agentSocketPath } from '../src/agent-socket';
import { expect, test, windowText, withStandIn } from './fixtures';
import type { Session } from './fixtures';
import { screenshotBothThemes } from './screenshots';

const token = 'cd'.repeat(32);

interface Fact {
  readonly entity_kind: string;
  readonly property: string;
  readonly value: unknown;
}

/** What the fake host answers, and what it has been asked. */
interface Host {
  facts: Fact[];
  version: number | null;
  /** The status of every answer; anything but 200 is a gear that is not ready. */
  status: number;
  /** Facts in a page, so that the list takes more than one call. */
  pageSize: number;
  readonly urls: string[];
  /** Whether collection is on, as the stand-in agent says it. */
  collecting: boolean;
  /** The stand-in agent cannot give its settings. */
  settingsFail: boolean;
  readonly port: () => number;
  /** Announces the host the way the agent does, in the runtime directory. */
  readonly announce: () => void;
  /** The host goes away and its file with it. */
  readonly withdraw: () => void;
  readonly stop: () => Promise<void>;
}

function newHost(session: Session): Host {
  const file = path.join(
    path.dirname(agentSocketPath(session.env, process.platform)),
    'connection.json',
  );
  const urls: string[] = [];
  const host: Host = {
    facts: [],
    version: 1,
    status: 200,
    pageSize: 2,
    urls,
    collecting: true,
    settingsFail: false,
    port: () => (server.address() as AddressInfo).port,
    announce: () => {
      mkdirSync(path.dirname(file), { recursive: true });
      writeFileSync(file, JSON.stringify({ port: host.port(), token }));
    },
    withdraw: () => {
      rmSync(file, { force: true });
    },
    stop: async () => {
      host.withdraw();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    },
  };
  const server: Server = createServer((request, response) => {
    const url = new URL(request.url ?? '', 'http://127.0.0.1');
    urls.push(`${url.pathname}${url.search}`);
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end('{}');
      return;
    }
    if (host.status !== 200) {
      response.writeHead(host.status).end('{}');
      return;
    }
    const limit = Math.min(Number(url.searchParams.get('limit') ?? '50'), host.pageSize);
    const from = Number(url.searchParams.get('cursor') ?? '0');
    const items = host.facts.slice(from, from + limit).map((fact, index) => ({
      ...fact,
      fact_id: `fact-${from + index}`,
      origin: { kind: 'agent' },
      stored_at: '2026-11-18T10:30:00Z',
    }));
    const next = from + limit < host.facts.length ? String(from + limit) : null;
    response.writeHead(200, { 'Content-Type': 'application/json' });
    response.end(
      JSON.stringify({
        items,
        page_info: { next_cursor: next, prev_cursor: null, limit },
        version: host.version,
      }),
    );
  });
  server.listen(0, '127.0.0.1');
  return host;
}

/** What the stand-in agent answers: its settings, with collection as the host says. */
function agentAnswer(host: Host): (line: string) => string {
  return (line) =>
    line.startsWith('settings-get') && host.settingsFail
      ? JSON.stringify({ error: 'settings unavailable' })
      : line.startsWith('settings-get')
        ? JSON.stringify({
            model: { base_url: '', model: '', api_key_set: false },
            collection: { enabled: host.collecting, claude_code: true, codex: true },
          })
        : JSON.stringify({ status: 'running', version: '0.1.0', pid: 42 });
}

async function inMemoryPage(
  session: Session,
  host: Host,
  check: (page: Page) => Promise<void>,
): Promise<void> {
  try {
    await withStandIn(session, agentAnswer(host), async (page) => {
      // The real agent is stopped by now and has taken its connection file with it.
      host.withdraw();
      await check(page);
    });
  } finally {
    await host.stop();
  }
}

const openMemory = (page: Page) => page.getByRole('button', { name: 'Memory', exact: true });

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

test('the token and the port of the host never reach the window', async ({ session }) => {
  const host = newHost(session);
  host.facts = profile;
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
      'getMemoryFacts',
      'getMemoryVersion',
    ]);
  });
});
