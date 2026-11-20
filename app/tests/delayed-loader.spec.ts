import type { Page } from '@playwright/test';
import { settingsReply } from './dashboard-shared';
import { expect, gate, test } from './fixtures';
import type { Session } from './fixtures';
import { newHost, withHostAndAgent } from './memory-host';
import { screenshotBothThemes } from './screenshots';

const conversation = 'c0ffee';
const secondConversation = 'c0ffef';
const lateMs = 1500;

/** The loader of each page, by the id it carries while it is on show. */
const loaders = [
  '#settings-loading',
  '#dashboard-reading',
  '#memory-reading',
  '#chat-loading',
  '#status-checking',
];

interface Failures {
  /** The agent cannot give its settings. */
  readonly settings?: string;
  /** The agent cannot give the state of the dashboard setup. */
  readonly setup?: string;
  /** The agent's answer to the status call, as the line it sends. */
  readonly status?: string;
}

function agentReply(line: string, failures: Failures): string {
  if (line === 'settings-get') {
    return failures.settings === undefined
      ? settingsReply(true)
      : JSON.stringify({ error: failures.settings });
  }
  if (line === 'dashboard-get') {
    return JSON.stringify({ document: null });
  }
  if (line === 'dashboard-setup-status') {
    if (failures.setup !== undefined) {
      return JSON.stringify({ error: failures.setup });
    }
    return JSON.stringify({ state: 'idle', sessions: 0, text_bytes: 0, sent: 0, received: 0 });
  }
  if (line === 'chat-list') {
    return JSON.stringify({
      conversations: [
        { conversation, title: 'An earlier talk', time: '2026-11-18T10:00:00Z' },
        { conversation: secondConversation, title: 'A later talk', time: '2026-11-18T11:00:00Z' },
      ],
    });
  }
  if (line.startsWith('chat-get ')) {
    const answer = line.endsWith(secondConversation)
      ? 'An answer from the later talk'
      : 'An answer from before';
    return [
      JSON.stringify({ kind: 'turn', role: 'user', text: 'A question from before' }),
      JSON.stringify({ kind: 'turn', role: 'assistant', text: answer }),
    ].join('\n');
  }
  return (
    failures.status ??
    JSON.stringify({ status: 'running', version: '0.1.0', pid: 42, model: 'example-model' })
  );
}

/** What a test changes about the stand-in agent. */
interface Slowness {
  /** Calls that fail. */
  readonly failures?: Failures;
  /** The wait of a call, when it is not `delayMs`. */
  readonly delayOf?: (line: string) => number | undefined;
  /** Calls that are answered only after a gate is released: the status, the settings, the setup. */
  readonly gates?: {
    readonly status?: Promise<void>;
    readonly settings?: Promise<void>;
    readonly setup?: Promise<void>;
  };
}

const nonStatusLines = ['settings-get', 'dashboard-get', 'dashboard-setup-status', 'chat-'];

/**
 * Opens the window on a stand-in agent that answers every call after `delayMs`, with a host for
 * the Memory page that answers at once, and runs `check` on it. The Memory page still waits, as
 * it reads the settings of the agent together with its facts. The window is watched for loaders
 * from its first load.
 */
async function withSlowAgent(
  session: Session,
  delayMs: number,
  check: (page: Page) => Promise<void>,
  slowness: Slowness = {},
): Promise<void> {
  // The Memory page waits with the rest: its facts and the settings are read together, and the
  // settings answer after the same wait.
  const host = newHost(session);
  await withHostAndAgent(
    session,
    host,
    async (line) => {
      const isStatus = !nonStatusLines.some((known) => line.startsWith(known));
      await new Promise((resolve) => setTimeout(resolve, slowness.delayOf?.(line) ?? delayMs));
      if (isStatus) {
        await slowness.gates?.status;
      }
      if (line === 'settings-get') {
        await slowness.gates?.settings;
      }
      if (line === 'dashboard-setup-status') {
        await slowness.gates?.setup;
      }
      return agentReply(line, slowness.failures ?? {});
    },
    async (page) => {
      host.announce();
      await watchLoaders(page);
      await page.reload();
      await check(page);
    },
  );
}

/**
 * Records every loader that is added to the page, from the start of each load, even one that is
 * gone at once.
 */
async function watchLoaders(page: Page): Promise<void> {
  await page.addInitScript((selector) => {
    const seen: string[] = [];
    Object.assign(window, { loadersSeen: seen });
    const note = (node: Node): void => {
      if (node instanceof Element) {
        const found = node.matches(selector) ? node : node.querySelector(selector);
        if (found !== null) {
          seen.push(found.id);
        }
      }
    };
    new MutationObserver((records) => {
      for (const record of records) {
        record.addedNodes.forEach(note);
      }
    }).observe(document, { childList: true, subtree: true });
  }, loaders.join(', '));
}

async function loadersSeen(page: Page): Promise<string[]> {
  return page.evaluate(() => Reflect.get(window, 'loadersSeen') as string[]);
}

let watchers = 0;

/** Calls `act` in the test at the moment the page adds the element, not a poll later. */
async function whenShown(page: Page, selector: string, act: () => void): Promise<void> {
  const callback = `shown${(watchers += 1)}`;
  await page.exposeFunction(callback, act);
  await page.evaluate(
    ({ id, name }) => {
      const look = new MutationObserver(() => {
        if (document.querySelector(id) !== null) {
          look.disconnect();
          (Reflect.get(window, name) as () => void)();
        }
      });
      look.observe(document.body, { childList: true, subtree: true });
    },
    { id: selector, name: callback },
  );
}

interface Frame {
  readonly loader: boolean;
  readonly first: boolean;
  readonly second: boolean;
}

/**
 * What the chat thread shows in each animation frame of the next `ms`: whether the conversation
 * loader is there, and whether the answers of the first and of the second conversation are. The
 * page samples itself, so a late poll of the test changes nothing.
 */
function sampleThread(page: Page, ms: number): Promise<Frame[]> {
  return page.evaluate(
    ({ duration, loader, first, second }) =>
      new Promise<Frame[]>((resolve) => {
        const frames: Frame[] = [];
        const start = performance.now();
        const sample = (): void => {
          // The text is in the page when its element is, whatever style it still fades in by.
          const text = document.body.textContent;
          frames.push({
            loader: document.querySelector(loader) !== null,
            first: text.includes(first),
            second: text.includes(second),
          });
          if (performance.now() - start < duration) {
            requestAnimationFrame(sample);
          } else {
            resolve(frames);
          }
        };
        sample();
      }),
    {
      duration: ms,
      loader: '#chat-loading',
      first: 'An answer from before',
      second: 'An answer from the later talk',
    },
  );
}

/**
 * Makes the page click, from the moment `text` shows there, each of `clicks` in turn, a frame
 * apart: `#id` is an element, anything else a button with that name.
 */
async function clickWhenTextAppears(
  page: Page,
  text: string,
  clicks: readonly string[],
): Promise<void> {
  await page.evaluate(
    ({ phrase, targets }) => {
      const look = new MutationObserver(() => {
        if (!document.body.textContent.includes(phrase)) {
          return;
        }
        look.disconnect();
        // A frame before each click: the chat takes a click only once it has drawn what the last
        // one did, as with a person's clicks.
        void (async () => {
          for (const target of targets) {
            await new Promise((resolve) => requestAnimationFrame(resolve));
            const element = target.startsWith('#')
              ? document.querySelector<HTMLElement>(target)
              : [...document.querySelectorAll('button')].find(
                  (button) => button.textContent.trim() === target,
                );
            element?.click();
          }
        })();
      });
      // The text may show by a change of style, with no node added.
      look.observe(document.body, {
        childList: true,
        subtree: true,
        characterData: true,
        attributes: true,
      });
    },
    { phrase: text, targets: clicks },
  );
}

const go = (page: Page, name: string) => page.getByRole('button', { name, exact: true }).click();

test('moving through the pages with an agent that answers at once shows no loader', async ({
  session,
}) => {
  // The status is held until the Status page is open, so the page starts with no status.
  const status = gate();
  await withSlowAgent(
    session,
    0,
    async (page) => {
      await go(page, 'Status');
      expect(await page.locator('#agent-status').count()).toBe(0);
      status.release();
      await expect(page.locator('#agent-status')).toHaveText('running');

      await go(page, 'Chat');
      await expect(page.locator('#chat-conversations li')).toHaveCount(2);
      await page.getByRole('button', { name: 'An earlier talk', exact: true }).click();
      await expect(page.getByText('An answer from before')).toBeVisible();

      await go(page, 'Dashboard');
      await expect(page.locator('#dashboard-none')).toBeVisible();

      await go(page, 'Memory');
      await expect(page.locator('#memory-empty')).toBeVisible();

      await go(page, 'Settings');
      await expect(page.locator('#setup-section-title')).toBeVisible();

      expect(await loadersSeen(page)).toEqual([]);
    },
    { gates: { status: status.promise } },
  );
});

test('with an agent that answers late each page shows its loader under its heading', async ({
  session,
}) => {
  await withSlowAgent(session, lateMs, async (page) => {
    await go(page, 'Status');
    await expect(page.getByRole('heading', { name: 'Construct agent', level: 1 })).toBeVisible();
    await expect(page.locator('#status-checking')).toBeVisible();
    await expect(page.locator('#agent-status')).toHaveText('running');

    await go(page, 'Dashboard');
    await expect(page.getByRole('heading', { name: 'Dashboard', level: 1 })).toBeVisible();
    await expect(page.locator('#dashboard-reading')).toBeVisible();
    await expect(page.locator('#dashboard-none')).toBeVisible();
    await expect(page.locator('#dashboard-reading')).toHaveCount(0);

    await go(page, 'Memory');
    await expect(page.getByRole('heading', { name: 'Memory', level: 1 })).toBeVisible();
    await expect(page.locator('#memory-reading')).toBeVisible();
    await expect(page.locator('#memory-empty')).toBeVisible();

    await go(page, 'Settings');
    await expect(page.getByRole('heading', { name: 'Settings', level: 1 })).toBeVisible();
    await expect(page.locator('#settings-loading')).toBeVisible();
    await expect(page.locator('#setup-section-title')).toBeVisible();

    await go(page, 'Chat');
    await page.getByRole('button', { name: 'An earlier talk', exact: true }).click();
    await expect(page.locator('#chat-loading')).toBeVisible();
    await expect(page.getByText('An answer from before')).toBeVisible();
    await expect(page.locator('#chat-loading')).toHaveCount(0);

    // The watcher of the test does see a loader that shows.
    expect(new Set(await loadersSeen(page))).toEqual(
      new Set(loaders.map((loader) => loader.slice(1))),
    );
  });
});

/**
 * Starts timing a loader in the page: from the moment it is added to the moment it is removed.
 * The page does it itself, so the time is not that of a poll of the test.
 */
function timeLoader(page: Page, selector: string): Promise<number> {
  return page.evaluate(
    (id) =>
      new Promise<number>((resolve) => {
        let shownAt: number | null = null;
        const look = new MutationObserver(() => {
          const there = document.querySelector(id) !== null;
          if (there && shownAt === null) {
            shownAt = performance.now();
          } else if (!there && shownAt !== null) {
            look.disconnect();
            resolve(performance.now() - shownAt);
          }
        });
        look.observe(document.body, { childList: true, subtree: true });
      }),
    selector,
  );
}

// A loader that goes with a failure that comes while it shows is on show for a moment, not for
// the half second that a held one would keep it.
const expectGoneWithTheAnswer = (ms: number): void => {
  expect(ms).toBeLessThan(300);
};

// The loader shows from 250 ms, and the answer comes before the 500 ms are over, so it is held
// until the minimum: it is on show for about 500 ms, whatever the poll of a test sees.
const expectHeldForHalfASecond = (ms: number): void => {
  expect(ms).toBeGreaterThanOrEqual(480);
  expect(ms).toBeLessThan(700);
};

test('the loader of the Dashboard stays for half a second, measured in the page', async ({
  session,
}) => {
  await withSlowAgent(session, 400, async (page) => {
    const shownFor = timeLoader(page, '#dashboard-reading');
    await go(page, 'Dashboard');
    expectHeldForHalfASecond(await shownFor);
  });
});

test('the skeleton of the Status page stays for half a second, measured in the page', async ({
  session,
}) => {
  const status = gate();
  await withSlowAgent(
    session,
    0,
    async (page) => {
      // The answer comes at the moment the skeleton shows.
      await whenShown(page, '#status-checking', status.release);
      const shownFor = timeLoader(page, '#status-checking');
      await go(page, 'Status');
      expectHeldForHalfASecond(await shownFor);
    },
    { gates: { status: status.promise } },
  );
});

test('the loader of a conversation stays for half a second, measured in the page', async ({
  session,
}) => {
  await withSlowAgent(
    session,
    0,
    async (page) => {
      await go(page, 'Chat');
      const shownFor = timeLoader(page, '#chat-loading');
      await page.getByRole('button', { name: 'An earlier talk', exact: true }).click();
      expectHeldForHalfASecond(await shownFor);
    },
    { delayOf: (line) => (line.endsWith(conversation) ? 400 : undefined) },
  );
});

test('a first answer that is a failure shows on the Status page at once', async ({ session }) => {
  const status = gate();
  await withSlowAgent(
    session,
    0,
    async (page) => {
      await whenShown(page, '#status-checking', status.release);
      const shownFor = timeLoader(page, '#status-checking');
      await go(page, 'Status');
      // The answer comes at the moment the skeleton shows, and the skeleton goes with it: a
      // failure held until the half second is over would keep it twice as long.
      expectGoneWithTheAnswer(await shownFor);
      await expect(page.locator('#agent-status')).toHaveText('unknown');
    },
    { gates: { status: status.promise }, failures: { status: '{"status":"confused"}' } },
  );
});

for (const part of ['database', 'construct']) {
  test(`a failed ${part} in a first answer shows on the Status page at once`, async ({
    session,
  }) => {
    const status = gate();
    await withSlowAgent(
      session,
      0,
      async (page) => {
        await whenShown(page, '#status-checking', status.release);
        const shownFor = timeLoader(page, '#status-checking');
        await go(page, 'Status');
        expectGoneWithTheAnswer(await shownFor);
        await expect(page.locator(`#${part}-state`)).toHaveText('failed');
      },
      {
        gates: { status: status.promise },
        failures: {
          status: JSON.stringify({
            status: 'running',
            version: '0.1.0',
            pid: 42,
            model: 'example-model',
            [part]: { state: 'failed', reason: 'it did not start' },
          }),
        },
      },
    );
  });
}

test('the Settings page waits for the state of the dashboard setup as well', async ({
  session,
}) => {
  const setup = gate();
  await withSlowAgent(
    session,
    0,
    async (page) => {
      await go(page, 'Settings');
      // The settings are there at once, so the loader that shows is the wait for the setup.
      await expect(page.locator('#settings-loading')).toBeVisible();
      expect(await page.locator('#setup-section-title').count()).toBe(0);
      setup.release();
      await expect(page.locator('#setup-section-title')).toBeVisible();
      await expect(page.locator('#settings-loading')).toHaveCount(0);
    },
    { gates: { setup: setup.promise } },
  );
});

test('a failure shows at once, and a loader that is on show gives way to it', async ({
  session,
}) => {
  const settings = gate();
  await withSlowAgent(
    session,
    0,
    async (page) => {
      // The answer comes at the moment the loader shows.
      await whenShown(page, '#settings-loading', settings.release);
      const shownFor = timeLoader(page, '#settings-loading');
      await go(page, 'Settings');
      expectGoneWithTheAnswer(await shownFor);
      await expect(
        page.getByRole('alert').filter({ hasText: 'settings unavailable' }),
      ).toBeVisible();
    },
    { failures: { settings: 'settings unavailable' }, gates: { settings: settings.promise } },
  );
});

test('a failed read of the dashboard setup shows at once, and the loader gives way to it', async ({
  session,
}) => {
  const setup = gate();
  await withSlowAgent(
    session,
    0,
    async (page) => {
      // The settings are there at once; the answer about the setup comes when the loader shows.
      await whenShown(page, '#settings-loading', setup.release);
      const shownFor = timeLoader(page, '#settings-loading');
      await go(page, 'Settings');
      expectGoneWithTheAnswer(await shownFor);
      await expect(page.locator('#setup-unavailable')).toBeVisible();
    },
    { failures: { setup: 'setup unavailable' }, gates: { setup: setup.promise } },
  );
});

test('a failure that comes at once shows no loader before it', async ({ session }) => {
  await withSlowAgent(
    session,
    0,
    async (page) => {
      await go(page, 'Settings');
      await expect(
        page.getByRole('alert').filter({ hasText: 'settings unavailable' }),
      ).toBeVisible();
      expect(await loadersSeen(page)).toEqual([]);
    },
    { failures: { settings: 'settings unavailable' } },
  );
});

test('a conversation that reads in 400 ms shows its messages with the loader beside them', async ({
  session,
}) => {
  await withSlowAgent(
    session,
    0,
    async (page) => {
      await go(page, 'Chat');
      const frames = sampleThread(page, 1200);
      await page.getByRole('button', { name: 'An earlier talk', exact: true }).click();
      // The messages are not held for the loader: it is on show beside them, for its half second.
      expect(await frames).toContainEqual({ loader: true, first: true, second: false });
    },
    { delayOf: (line) => (line.endsWith(conversation) ? 400 : undefined) },
  );
});

test('opening a second conversation does not take over the loader of the first', async ({
  session,
}) => {
  await withSlowAgent(
    session,
    0,
    async (page) => {
      await go(page, 'Chat');
      // The page opens the second conversation the moment the first shows, inside the half
      // second that the loader of the first is held, and it is read at once.
      await clickWhenTextAppears(page, 'An answer from before', ['A later talk']);
      const frames = sampleThread(page, 1500);
      await page.getByRole('button', { name: 'An earlier talk', exact: true }).click();
      const sampled = await frames;
      expect(sampled.some((frame) => frame.second)).toBe(true);
      expect(sampled.filter((frame) => frame.loader && frame.second)).toEqual([]);
    },
    { delayOf: (line) => (line.endsWith(conversation) ? 400 : undefined) },
  );
});

test('a conversation opened again after the loader was dropped starts its own wait', async ({
  session,
}) => {
  let reads = 0;
  await withSlowAgent(
    session,
    0,
    async (page) => {
      await go(page, 'Chat');
      // The page drops the loader with a new chat the moment the first read shows, and opens the
      // same conversation again, which is read in 100 ms: nothing is left from the first wait.
      await clickWhenTextAppears(page, 'An answer from before', ['#chat-new', 'An earlier talk']);
      const frames = sampleThread(page, 1500);
      await page.getByRole('button', { name: 'An earlier talk', exact: true }).click();
      const sampled = await frames;
      // From the new chat (the first frame without the answer after it showed) on, the conversation
      // is opened again and read in 100 ms: it shows, and shows no loader.
      const shown = sampled.findIndex((frame) => frame.first);
      const dropped = sampled.findIndex((frame, index) => index > shown && !frame.first);
      const after = sampled.slice(dropped);
      expect(dropped).toBeGreaterThan(shown);
      expect(after.some((frame) => frame.first)).toBe(true);
      expect(after.filter((frame) => frame.loader)).toEqual([]);
    },
    {
      delayOf: (line) => {
        if (!line.endsWith(conversation) || !line.startsWith('chat-get')) {
          return undefined;
        }
        reads += 1;
        // The first read is slow enough for the loader to show; the second is not.
        return reads === 1 ? 400 : 100;
      },
    },
  );
});

test('the loader of a late answer, in both themes', async ({ session }) => {
  await withSlowAgent(session, 3000, async (page) => {
    await go(page, 'Settings');
    await expect(page.locator('#settings-loading')).toBeVisible();
    await screenshotBothThemes(page, 'delayed-loader');
    // The loader was on show for both pictures.
    await expect(page.locator('#settings-loading')).toBeVisible();
  });
});
