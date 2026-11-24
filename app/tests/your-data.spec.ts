import type { Page } from '@playwright/test';
import { rows } from './chat-helpers';
import { settingsReply } from './dashboard-shared';
import { expect, openSettings, test } from './fixtures';
import type { Session } from './fixtures';
import { newHost, openMemory, withHostAndAgent } from './memory-host';
import type { Fact, Host } from './memory-host';
import { screenshotBothThemes } from './screenshots';

/** What the stand-in agent keeps and has been asked. */
interface Agent {
  chats: { id: string; title: string }[];
  collecting: boolean;
  /** The answer to `memory-erase` when it is not a start. */
  eraseError: string | null;
  /** The agent has wiped its chats and switched collection off before it gives `eraseError`. */
  eraseWipesFirst: boolean;
  /** The host has started to erase though the agent answers `eraseError`: one of its own stores failed. */
  eraseLeavesGearErasing: boolean;
  /** The answer to `chat-delete all` when it is not a count. */
  chatDeleteError: string | null;
  /** The answer to `chat-list` when it is not the list. */
  listError: string | null;
  /** The phones the agent lists as paired. */
  phones: { id: string; name: string; paired_at: number }[];
  /** The answer to `phone-list` when it is not the list. */
  phonesError: string | null;
  readonly lines: string[];
}

const profile: Fact[] = [
  { entity_kind: 'identity', property: 'full_name', value: 'Ada Lovelace' },
  { entity_kind: 'role', property: 'job_title', value: 'Engineer' },
];

/** An agent that wiped its chats, started the gear's erasure, and could not erase one of its own stores. */
function halfErased(agent: Agent): void {
  agent.eraseError =
    'collection is off and the erasure started, but the agent could not erase: sessions; run it again when the erasure has ended';
  agent.eraseWipesFirst = true;
  agent.eraseLeavesGearErasing = true;
}

function newAgent(): Agent {
  return {
    chats: [
      { id: '0a0a0a0a-0000-4000-8000-000000000001', title: 'Rust questions' },
      { id: '0a0a0a0a-0000-4000-8000-000000000002', title: 'Weekly plan' },
    ],
    collecting: true,
    eraseError: null,
    eraseWipesFirst: false,
    eraseLeavesGearErasing: false,
    chatDeleteError: null,
    listError: null,
    phones: [],
    phonesError: null,
    lines: [],
  };
}

/** Answers like the agent: its settings, its chats, and the two deletions the page can ask for. */
function reply(agent: Agent, host: Host, line: string): string {
  agent.lines.push(line);
  if (line === 'settings-get') {
    return settingsReply(agent.collecting);
  }
  if (line === 'chat-list') {
    if (agent.listError !== null) {
      return JSON.stringify({ error: agent.listError });
    }
    return JSON.stringify({
      conversations: agent.chats.map((chat) => ({
        conversation: chat.id,
        title: chat.title,
        time: '2026-11-18T10:30:00Z',
      })),
    });
  }
  if (line.startsWith('phone-list ')) {
    if (agent.phonesError !== null) {
      return JSON.stringify({ error: agent.phonesError });
    }
    return JSON.stringify({ phones: agent.phones, next: null });
  }
  if (line === 'chat-delete all') {
    if (agent.chatDeleteError !== null) {
      return JSON.stringify({ error: agent.chatDeleteError });
    }
    const deleted = agent.chats.length;
    agent.chats = [];
    return JSON.stringify({ deleted });
  }
  if (line.startsWith('chat-delete ')) {
    const before = agent.chats.length;
    agent.chats = agent.chats.filter((chat) => chat.id !== line.slice('chat-delete '.length));
    return JSON.stringify({ deleted: before - agent.chats.length });
  }
  if (line.startsWith('chat-get ')) {
    return JSON.stringify({ kind: 'turn', role: 'user', text: 'hello there' });
  }
  if (line === 'memory-erase') {
    // Asked again while the gear erases, the agent changes nothing.
    if (host.erasing) {
      return JSON.stringify({ erasure: 'running', collection_enabled: false });
    }
    if (agent.eraseError !== null && !agent.eraseWipesFirst) {
      return JSON.stringify({ error: agent.eraseError });
    }
    agent.collecting = false;
    agent.chats = [];
    host.erasing = true;
    if (agent.eraseError !== null) {
      host.erasing = agent.eraseLeavesGearErasing;
      return JSON.stringify({ error: agent.eraseError });
    }
    return JSON.stringify({ erasure: 'started', collection_enabled: false });
  }
  if (line === 'dashboard-setup-status') {
    return JSON.stringify({
      state: 'idle',
      sessions: 0,
      text_bytes: 0,
      sent: 0,
      received: 0,
    });
  }
  return JSON.stringify({ status: 'running', version: '0.1.0', pid: 42 });
}

/** A second past the 3 minutes the page waits for the gear. */
const pastThreeMinutesMs = 181_000;

/** How many times the page has read the gear's settings route. */
const settingsLooks = (host: Host): number =>
  host.urls.filter((url) => url.endsWith('/settings')).length;

/**
 * Presses Delete everything and confirms, and waits until the page has looked at the gear
 * `looksAfter` times more: the agent's answer has come and the page waits for the gear.
 */
async function pressErase(page: Page, host: Host, looksAfter: number): Promise<void> {
  const looks = settingsLooks(host);
  await page.locator('#erase-everything').click();
  await page.locator('#erase-everything-confirm').click();
  await expect.poll(() => settingsLooks(host)).toBeGreaterThanOrEqual(looks + looksAfter);
}

/**
 * Presses Delete everything with the clock under the test's control, lets the waiting page look
 * at the gear, and runs the clock to the end of its 3 minutes: the page then says it gave up.
 */
async function eraseUntilGivenUp(page: Page, host: Host): Promise<void> {
  await page.clock.install();
  // The first look of the waiting page has been made, so its 3 minutes have begun.
  await pressErase(page, host, 1);
  await expect(page.locator('#erase-running')).toBeVisible();
  await page.clock.fastForward(pastThreeMinutesMs);
  await expect(page.locator('#erase-error')).toContainText('still erasing after 3 minutes');
}

const inYourData = (
  session: Session,
  agent: Agent,
  host: Host,
  check: (page: Page) => Promise<void>,
) =>
  withHostAndAgent(
    session,
    host,
    (line) => reply(agent, host, line),
    async (page) => {
      host.announce();
      await check(page);
    },
  );

const openChat = (page: Page) => page.getByRole('button', { name: 'Chat', exact: true });

/** The section at the top of the window, so a screenshot shows the section and not the forms above. */
async function shoot(page: Page, name: string): Promise<void> {
  await page.locator('#your-data-title').evaluate((heading) => {
    heading.scrollIntoView({ block: 'start' });
  });
  await screenshotBothThemes(page, name);
}

test('the sidebar of the chat page has no Delete all, and a chat can still be deleted from the list', async ({
  session,
}) => {
  const host = newHost(session);
  await inYourData(session, newAgent(), host, async (page) => {
    await expect(rows(page)).toHaveText(['Rust questions', 'Weekly plan']);
    await expect(page.locator('#chat-delete-all')).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Delete all' })).toHaveCount(0);
    await page.getByRole('button', { name: 'Delete conversation: Weekly plan' }).click();
    await expect(rows(page)).toHaveText(['Rust questions']);
  });
});

test('Delete all chats in Settings removes the chats and keeps the facts', async ({ session }) => {
  const host = newHost(session);
  host.facts = [...profile];
  const agent = newAgent();
  await inYourData(session, agent, host, async (page) => {
    await openSettings(page, 'Your data');
    await shoot(page, 'your-data-section');

    // Cancel changes nothing.
    await page.locator('#chat-delete-all').click();
    await expect(
      page.getByRole('alertdialog', { name: 'Delete all conversations?' }),
    ).toBeVisible();
    await page.locator('#chat-delete-all-cancel').click();
    expect(agent.lines).not.toContain('chat-delete all');
    expect(agent.chats).toHaveLength(2);

    await page.locator('#chat-delete-all').click();
    await page.locator('#chat-delete-all-confirm').click();
    await expect(page.locator('#chat-delete-all')).toBeDisabled();
    expect(agent.lines.filter((line) => line === 'chat-delete all')).toHaveLength(1);
    expect(agent.lines).not.toContain('memory-erase');

    await openChat(page).click();
    await expect(rows(page)).toHaveCount(0);

    // The facts, the sessions and the settings are not touched.
    host.announce();
    await openMemory(page).click();
    await expect(
      page.locator('#memory-tree').getByText('Ada Lovelace', { exact: true }),
    ).toBeVisible();
    expect(host.deletes).toEqual([]);
    expect(agent.collecting).toBe(true);
  });
});

test('Delete everything: Cancel keeps all; Confirm erases, waits for the gear and ends with collection off', async ({
  session,
}) => {
  test.setTimeout(90_000);
  const host = newHost(session);
  host.facts = [...profile];
  const agent = newAgent();
  await inYourData(session, agent, host, async (page) => {
    // A chat is open when everything is deleted; the wipe takes it away.
    await rows(page).first().getByRole('button').first().click();
    await expect(page.locator('[data-role=user]')).toHaveText(['hello there']);
    await openSettings(page, 'Your data');
    const collection = page.locator('#collection-enabled');
    await expect(collection).toBeChecked();

    await page.locator('#erase-everything').click();
    const dialog = page.getByRole('alertdialog', { name: 'Delete everything?' });
    await expect(dialog).toContainText('facts');
    await expect(dialog).toContainText('the sessions Construct collected');
    await expect(dialog).toContainText('activity time');
    await expect(dialog).toContainText('all your chats');
    await expect(dialog).toContainText('The model and the settings stay.');
    await expect(dialog).toContainText('This cannot be undone.');
    await screenshotBothThemes(page, 'your-data-confirm');

    await page.locator('#erase-everything-cancel').click();
    await expect(dialog).toHaveCount(0);
    expect(agent.lines).not.toContain('memory-erase');
    expect(agent.chats).toHaveLength(2);
    await expect(collection).toBeChecked();

    await page.locator('#erase-everything').click();
    await page.locator('#erase-everything-confirm').click();
    // The agent has started the erasure and the gear still says it runs: the loader stays.
    await expect(page.locator('#erase-running')).toBeVisible();
    await expect(page.locator('#erase-done')).toHaveCount(0);
    await expect(page.locator('#erase-everything')).toBeDisabled();
    await expect(page.locator('#chat-delete-all')).toBeDisabled();
    expect(agent.lines.filter((line) => line === 'memory-erase')).toHaveLength(1);
    await expect.poll(() => settingsLooks(host)).toBeGreaterThan(1);
    await expect(page.locator('#erase-running')).toBeVisible();
    await expect(collection).not.toBeChecked();
    await shoot(page, 'your-data-erasing');

    host.erasing = false;
    await expect(page.locator('#erase-done')).toContainText('collection was switched off');
    await expect(page.locator('#erase-done')).toContainText('Collect from my coding tools');
    await expect(page.locator('#erase-running')).toHaveCount(0);
    await expect(page.locator('#erase-everything')).toBeEnabled();
    await shoot(page, 'your-data-done');

    await openChat(page).click();
    await expect(rows(page)).toHaveCount(0);
    await expect(page.locator('#chat-empty')).toBeVisible();
  });
});

test('a refusal of the agent shows its reason, and a gear that cannot be read ends the wait with its reason', async ({
  session,
}) => {
  test.setTimeout(90_000);
  const host = newHost(session);
  const agent = newAgent();
  agent.eraseError =
    'collection is off, but the erasure did not start: the Construct host cannot be reached';
  await inYourData(session, agent, host, async (page) => {
    await openSettings(page, 'Your data');
    await page.locator('#erase-everything').click();
    await page.locator('#erase-everything-confirm').click();
    await expect(page.locator('#erase-error')).toContainText(
      'the erasure did not start: the Construct host cannot be reached',
    );
    await expect(page.locator('#erase-running')).toHaveCount(0);
    await expect(page.locator('#erase-done')).toHaveCount(0);
    await shoot(page, 'your-data-failed');

    // The agent starts it, but the gear cannot be asked how it goes.
    agent.eraseError = null;
    host.status = 500;
    await page.locator('#erase-everything').click();
    await page.locator('#erase-everything-confirm').click();
    await expect(page.locator('#erase-error')).toContainText('Construct is not ready');
    await expect(page.locator('#erase-running')).toHaveCount(0);
    await expect(page.locator('#erase-done')).toHaveCount(0);
  });
});

test('a refusal of Delete all chats shows its reason in plain words and keeps the list', async ({
  session,
}) => {
  const host = newHost(session);
  const agent = newAgent();
  agent.chatDeleteError = 'busy';
  await inYourData(session, agent, host, async (page) => {
    await openSettings(page, 'Your data');
    await page.locator('#chat-delete-all').click();
    await page.locator('#chat-delete-all-confirm').click();
    await expect(page.locator('#chats-error')).toContainText('Another answer is still running.');
    await openChat(page).click();
    await expect(rows(page)).toHaveCount(2);
  });
});

test('chats the agent wiped before it refused to erase are gone from the list too', async ({
  session,
}) => {
  const host = newHost(session);
  const agent = newAgent();
  agent.eraseError =
    'collection is off, but the erasure did not start: the Construct host cannot be reached';
  agent.eraseWipesFirst = true;
  await inYourData(session, agent, host, async (page) => {
    await expect(rows(page)).toHaveCount(2);
    await openSettings(page, 'Your data');
    await page.locator('#erase-everything').click();
    await page.locator('#erase-everything-confirm').click();
    await expect(page.locator('#erase-error')).toContainText('the erasure did not start');
    await expect(page.locator('#collection-enabled')).not.toBeChecked();
    await openChat(page).click();
    await expect(rows(page)).toHaveCount(0);
  });
});

test('leaving Settings while the gear erases does not lose the wait', async ({ session }) => {
  test.setTimeout(90_000);
  const host = newHost(session);
  const agent = newAgent();
  await inYourData(session, agent, host, async (page) => {
    await openSettings(page, 'Your data');
    await page.locator('#erase-everything').click();
    await page.locator('#erase-everything-confirm').click();
    await expect(page.locator('#erase-running')).toBeVisible();

    // The chats are gone from the list at once, though the gear has not ended.
    await openChat(page).click();
    await expect(rows(page)).toHaveCount(0);

    // Back on Settings the page looks at the gear and waits again, with the button off.
    await openSettings(page, 'Your data');
    await expect(page.locator('#erase-running')).toBeVisible();
    await expect(page.locator('#erase-everything')).toBeDisabled();

    host.erasing = false;
    await expect(page.locator('#erase-done')).toBeVisible();
    await expect(page.locator('#erase-running')).toHaveCount(0);
  });
});

test('an agent that could not erase all of its own data is not followed by "Everything is deleted"', async ({
  session,
}) => {
  test.setTimeout(90_000);
  const host = newHost(session);
  const agent = newAgent();
  halfErased(agent);
  await inYourData(session, agent, host, async (page) => {
    await openSettings(page, 'Your data');
    // The agent has answered with an error and the gear is erasing: the page looks once for that
    // and then waits, so a second look of the waiting page has been made.
    await pressErase(page, host, 2);
    await expect(page.locator('#erase-running')).toBeVisible();
    host.erasing = false;
    await expect(page.locator('#erase-error')).toContainText('the agent could not erase: sessions');
    await expect(page.locator('#erase-done')).toHaveCount(0);
    await expect(page.locator('#erase-running')).toHaveCount(0);
  });
});

test('a gear that keeps erasing ends the wait after three minutes with a reason', async ({
  session,
}) => {
  test.setTimeout(90_000);
  const host = newHost(session);
  const agent = newAgent();
  await inYourData(session, agent, host, async (page) => {
    await openSettings(page, 'Your data');
    await eraseUntilGivenUp(page, host);
    await expect(page.locator('#erase-running')).toHaveCount(0);
    await expect(page.locator('#erase-done')).toHaveCount(0);
  });
});

test('the end of an erasure is kept when Settings is left and shown again', async ({ session }) => {
  test.setTimeout(90_000);
  const host = newHost(session);
  const agent = newAgent();
  halfErased(agent);
  await inYourData(session, agent, host, async (page) => {
    await openSettings(page, 'Your data');
    await pressErase(page, host, 2);

    // Away from Settings the wait goes on, with the agent's reason kept for its end.
    const waiting = settingsLooks(host);
    await openChat(page).click();
    host.erasing = false;
    await expect.poll(() => settingsLooks(host)).toBeGreaterThan(waiting);
    await openSettings(page, 'Your data');
    await expect(page.locator('#erase-error')).toContainText('the agent could not erase: sessions');
    await expect(page.locator('#erase-done')).toHaveCount(0);
  });
});

test('a gear that erases while the app was started without its host is found at the next visit to Settings', async ({
  session,
}) => {
  test.setTimeout(90_000);
  const host = newHost(session);
  const agent = newAgent();
  host.erasing = true;
  await withHostAndAgent(
    session,
    host,
    (line) => reply(agent, host, line),
    async (page) => {
      // No host is announced at launch: the page is idle, with nothing to wait for.
      await openSettings(page, 'Your data');
      await expect(page.locator('#erase-running')).toHaveCount(0);
      await expect(page.locator('#erase-everything')).toBeEnabled();

      host.announce();
      await openChat(page).click();
      await openSettings(page, 'Your data');
      await expect(page.locator('#erase-running')).toBeVisible();
      await expect(page.locator('#erase-everything')).toBeDisabled();
      await expect(page.locator('#chat-delete-all')).toBeDisabled();

      host.erasing = false;
      await expect(page.locator('#erase-done')).toBeVisible();
      expect(agent.lines).not.toContain('memory-erase');
    },
  );
});

test('a wait that ran out is taken up again at the next visit while the gear still erases', async ({
  session,
}) => {
  test.setTimeout(90_000);
  const host = newHost(session);
  const agent = newAgent();
  await inYourData(session, agent, host, async (page) => {
    await openSettings(page, 'Your data');
    await eraseUntilGivenUp(page, host);

    await openChat(page).click();
    await openSettings(page, 'Your data');
    await expect(page.locator('#erase-running')).toBeVisible();
    host.erasing = false;
    await expect(page.locator('#erase-done')).toBeVisible();
  });
});

test("a second press after the wait ran out still ends in the agent's reason", async ({
  session,
}) => {
  test.setTimeout(90_000);
  const host = newHost(session);
  const agent = newAgent();
  halfErased(agent);
  await inYourData(session, agent, host, async (page) => {
    await openSettings(page, 'Your data');
    await eraseUntilGivenUp(page, host);

    // The gear still erases, so the agent answers `running` to a second press.
    await page.locator('#erase-everything').click();
    await page.locator('#erase-everything-confirm').click();
    await expect(page.locator('#erase-running')).toBeVisible();
    host.erasing = false;
    await expect(page.locator('#erase-error')).toContainText('the agent could not erase: sessions');
    await expect(page.locator('#erase-done')).toHaveCount(0);
  });
});

test('an error before anything was erased keeps the open chat open', async ({ session }) => {
  const host = newHost(session);
  const agent = newAgent();
  agent.eraseError = 'the Construct host is not ready yet';
  await inYourData(session, agent, host, async (page) => {
    await rows(page).first().getByRole('button').first().click();
    await expect(page.locator('[data-role=user]')).toHaveText(['hello there']);

    await openSettings(page, 'Your data');
    await page.locator('#erase-everything').click();
    await page.locator('#erase-everything-confirm').click();
    await expect(page.locator('#erase-error')).toContainText('the Construct host is not ready yet');
    await openChat(page).click();
    await expect(rows(page)).toHaveCount(2);
    await expect(page.locator('[data-role=user]')).toHaveText(['hello there']);
  });
});

test('the list cannot be read after the agent erased: the chat page says why', async ({
  session,
}) => {
  const host = newHost(session);
  const agent = newAgent();
  await inYourData(session, agent, host, async (page) => {
    await rows(page).first().getByRole('button').first().click();
    await expect(page.locator('[data-role=user]')).toHaveText(['hello there']);
    await openSettings(page, 'Your data');
    agent.listError = 'the chats cannot be listed';
    await page.locator('#erase-everything').click();
    await page.locator('#erase-everything-confirm').click();
    await expect(page.locator('#erase-running')).toBeVisible();
    await openChat(page).click();
    await expect(page.locator('#chat-notice')).toContainText('the chats cannot be listed');
    // The list could not be read, so nothing says the open chat is gone: it stays open.
    await expect(page.locator('[data-role=user]')).toHaveText(['hello there']);
  });
});

test('the agent’s reason survives a failed look at the gear and a visit, and a clean erasure after it ends in done', async ({
  session,
}) => {
  test.setTimeout(90_000);
  const host = newHost(session);
  const agent = newAgent();
  halfErased(agent);
  await inYourData(session, agent, host, async (page) => {
    await openSettings(page, 'Your data');
    await pressErase(page, host, 2);

    // The gear cannot be read: the wait ends with that, and the agent's reason is kept.
    host.status = 500;
    await expect(page.locator('#erase-error')).toContainText('Construct is not ready');

    // The gear is back and still erasing; a visit takes the wait up again.
    host.status = 200;
    await openChat(page).click();
    await openSettings(page, 'Your data');
    await expect(page.locator('#erase-running')).toBeVisible();
    host.erasing = false;
    await expect(page.locator('#erase-error')).toContainText('the agent could not erase: sessions');
    await expect(page.locator('#erase-done')).toHaveCount(0);

    // Run again, and the agent erases cleanly: the old reason does not come back.
    agent.eraseError = null;
    agent.eraseWipesFirst = false;
    agent.eraseLeavesGearErasing = false;
    await page.locator('#erase-everything').click();
    await page.locator('#erase-everything-confirm').click();
    await expect(page.locator('#erase-running')).toBeVisible();
    host.erasing = false;
    await expect(page.locator('#erase-done')).toBeVisible();
    await expect(page.locator('#erase-error')).toHaveCount(0);
  });
});

test('the confirmation of Delete everything says the paired phones are emptied too, only when one is paired', async ({
  session,
}) => {
  const host = newHost(session);
  const agent = newAgent();
  agent.phones = [{ id: 'first', name: "Anna's phone", paired_at: 1735718400 }];
  await inYourData(session, agent, host, async (page) => {
    await openSettings(page, 'Your data');
    await page.locator('#erase-everything').click();
    await expect(page.getByRole('alertdialog', { name: 'Delete everything?' })).toBeVisible();
    await expect(page.locator('#erase-everything-phones')).toHaveText(
      'The phones paired with this computer are emptied too, the next time each of them connects.',
    );
    await screenshotBothThemes(page, 'delete-everything-with-a-phone');
    await page.locator('#erase-everything-cancel').click();
    await expect(page.locator('#erase-everything-body')).toHaveCount(0);

    // The last phone is unpaired: the next confirmation has no such line.
    agent.phones = [];
    await page.locator('#erase-everything').click();
    await expect(page.locator('#erase-everything-body')).toBeVisible();
    await expect(page.locator('#erase-everything-phones')).toHaveCount(0);
    await screenshotBothThemes(page, 'delete-everything-with-no-phone');
  });
});

test('the confirmation leaves the phones line out when the agent cannot list the phones', async ({
  session,
}) => {
  const agent = newAgent();
  agent.phones = [{ id: 'first', name: "Anna's phone", paired_at: 1735718400 }];
  agent.phonesError = 'no phones';
  await inYourData(session, agent, newHost(session), async (page) => {
    await openSettings(page, 'Your data');
    await page.locator('#erase-everything').click();
    await expect(page.locator('#erase-everything-body')).toBeVisible();
    await expect(page.locator('#erase-everything-phones')).toHaveCount(0);
  });
});
