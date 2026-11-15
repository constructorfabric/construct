import { execFileSync } from 'node:child_process';
import type { Page } from '@playwright/test';
import {
  agentBinary,
  captureTestOutput,
  expectNoPolicyViolations,
  expect,
  openApp,
  test as base,
  windowText,
} from './fixtures';
import { closedPort, goodKey, goodModel, providerFixture } from './provider';
import type { Provider } from './provider';
import { screenshotBothThemes } from './screenshots';

const reasonDeadlineMs = 15_000;
const stopDeadlineMs = 2000;

const test = base.extend<{ provider: Provider }>(providerFixture);

async function configure(
  page: Page,
  fields: { address: string; model?: string; key?: string },
): Promise<void> {
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByLabel('Address').fill(fields.address);
  await page.getByLabel('Model name').fill(fields.model ?? goodModel);
  await page.getByLabel('API key', { exact: true }).fill(fields.key ?? goodKey);
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.locator('#model-outcome')).toContainText('Saved');
  await page.getByRole('button', { name: 'Chat', exact: true }).click();
}

const input = (page: Page) => page.locator('#chat-input');
const assistant = (page: Page) => page.locator('[data-role=assistant]');
const user = (page: Page) => page.locator('[data-role=user]');
const turns = (page: Page) => page.locator('[data-role]');
const rows = (page: Page) => page.locator('#chat-conversations li');

async function ask(page: Page, message: string): Promise<void> {
  await input(page).fill(message);
  await input(page).press('Enter');
}

async function atBottom(page: Page): Promise<boolean> {
  return page.locator('#chat-thread').evaluate((element) => {
    return element.scrollHeight - element.scrollTop - element.clientHeight <= 48;
  });
}

test('Chat is the first page and opens by default, with two example questions', async ({
  page,
}) => {
  await expect(
    page.getByRole('navigation', { name: 'Pages' }).getByRole('button').first(),
  ).toHaveText('Chat');
  await expect(page.locator('#chat-empty')).toBeVisible();
  await expect(page.locator('#chat-empty button')).toHaveText([
    'What do you know about me?',
    'I mostly write Rust',
  ]);
  await expect(input(page)).toBeFocused();
  await expect(page.locator('#chat-send-button')).toBeDisabled();
  await expect(page.locator('#chat-new')).toBeVisible();
  await expect(page.locator('#chat-delete-all')).toHaveCount(0);
  await screenshotBothThemes(page, 'chat-empty');
});

test('with no model set the page links to Settings, before and after a message', async ({
  page,
}) => {
  await expect(page.locator('#chat-no-model')).toBeVisible({ timeout: 10_000 });
  await page.locator('#chat-open-settings').click();
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  await page.getByRole('button', { name: 'Chat', exact: true }).click();

  await ask(page, 'hello');
  await expect(page.locator('#chat-failure')).toContainText('No model is set');
  await expect(page.locator('#chat-no-model')).toHaveCount(0);
  await expect(user(page)).toHaveText(['hello']);
  await page.locator('#chat-failure-settings').click();
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  await page.getByRole('button', { name: 'Chat', exact: true }).click();

  // Try again sends the same message once more, and the thread keeps one copy of it.
  await page.locator('#chat-retry').click();
  await expect(page.locator('#chat-failure')).toContainText('No model is set');
  await expect(user(page)).toHaveText(['hello']);
});

test('the answer streams in, Enter sends and Shift+Enter is a new line', async ({
  page,
  provider,
}) => {
  provider.script = {
    chunks: ['Hello ', 'from ', 'the agent, ', 'and the end.'],
    pauseMs: 200,
    holdAt: 3,
  };
  await configure(page, { address: provider.url });

  await input(page).fill('first line');
  await input(page).press('Shift+Enter');
  await input(page).pressSequentially('second line');
  await expect(input(page)).toHaveValue('first line\nsecond line');
  expect(provider.seen.length).toBe(0);
  await input(page).press('Enter');

  // Text is on screen before the provider has sent its last piece.
  await expect(assistant(page)).toContainText('Hello from the agent,');
  expect(provider.sent()).toBe(3);
  await expect(assistant(page)).not.toContainText('and the end.');
  await expect(page.locator('#chat-stop-button')).toBeVisible();
  await expect(user(page)).toContainText('first line');
  await expect(user(page)).toContainText('second line');

  // Send waits for the answer: typing is allowed, sending is not.
  await input(page).fill('next');
  await expect(page.locator('#chat-send-button')).toBeDisabled();
  await input(page).press('Enter');
  expect(provider.seen.length).toBe(1);

  provider.release();
  await expect(assistant(page)).toHaveText('Hello from the agent, and the end.');
  await expect(page.locator('#chat-stop-button')).toHaveCount(0);
  await expect(page.locator('#chat-failure')).toHaveCount(0);
  // The conversation is named after its first message.
  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page).first()).toContainText('first line');
  await expect(page.locator('#chat-send-button')).toBeEnabled();
});

test('the thread follows the answer down only while the reader is at the bottom', async ({
  page,
  provider,
}) => {
  const lines = Array.from({ length: 60 }, (_, index) => `line ${index}\n`);
  provider.script = { chunks: lines, pauseMs: 20, holdAt: 30 };
  await configure(page, { address: provider.url });

  await ask(page, 'count');
  await expect(assistant(page)).toContainText('line 29');
  expect(provider.sent()).toBe(30);
  await expect.poll(() => atBottom(page)).toBe(true);
  const thread = page.locator('#chat-thread');
  expect(await thread.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(
    true,
  );

  // The reader scrolls up to read; the rest of the answer must not pull them down.
  await thread.evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect.poll(() => atBottom(page)).toBe(false);
  provider.release();
  await expect(assistant(page)).toContainText('line 59');
  expect(await thread.evaluate((element) => element.scrollTop)).toBeLessThan(100);

  // A reader who is back at the bottom is followed again.
  provider.script = { chunks: lines, pauseMs: 5 };
  await thread.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await ask(page, 'again');
  await expect(assistant(page).last()).toContainText('line 59');
  await expect.poll(() => atBottom(page)).toBe(true);
});

test('Stop ends the answer within 2 seconds, the part stays, and Try again asks again', async ({
  page,
  provider,
}) => {
  provider.script = {
    chunks: ['Part one ', 'part two ', 'never'],
    pauseMs: 50,
    holdAt: 2,
    forever: true,
  };
  await configure(page, { address: provider.url });

  await ask(page, 'long answer please');
  await expect(assistant(page)).toContainText('Part one part two');

  await page.locator('#chat-stop-button').click();
  await expect(page.locator('#chat-failure')).toContainText('Stopped', { timeout: stopDeadlineMs });
  await expect(assistant(page)).toHaveText('Part one part two');
  await expect(page.locator('#chat-stop-button')).toHaveCount(0);
  // The agent let go of the provider, so the request is closed there too.
  await expect.poll(() => provider.aborted()).toBe(1);

  provider.script = { chunks: ['Second ', 'try.'], pauseMs: 0 };
  await page.locator('#chat-retry').click();
  await expect(assistant(page).last()).toHaveText('Second try.');
  await expect(page.locator('#chat-failure')).toHaveCount(0);
  expect(provider.seen.length).toBe(2);

  // What the agent stored: the stopped part is in the conversation, read back with chat-get.
  await page.locator('#chat-new').click();
  await rows(page).first().getByRole('button').first().click();
  await expect(turns(page)).toHaveText([
    'long answer please',
    'Part one part two',
    'long answer please',
    'Second try.',
  ]);
});

test('after closing the app and the agent and starting both again the conversation is back', async ({
  session,
  provider,
}) => {
  provider.script = { chunks: ['Rust ', 'is fine.'], pauseMs: 0 };
  const first = await openApp(session);
  try {
    await configure(first.page, { address: provider.url });
    await ask(first.page, 'what do I write');
    await expect(assistant(first.page)).toHaveText('Rust is fine.');
    await expect(rows(first.page)).toHaveCount(1);
    await expectNoPolicyViolations(first.page);
  } finally {
    await first.app.close();
  }
  await session.stopAgent();
  await session.startAgent();

  const second = await openApp(session);
  try {
    await expect(rows(second.page)).toHaveCount(1);
    await expect(rows(second.page).first()).toContainText('what do I write');
    await expect(second.page.locator('#chat-empty')).toBeVisible();
    await rows(second.page).first().getByRole('button').first().click();
    await expect(user(second.page)).toHaveText(['what do I write']);
    await expect(assistant(second.page)).toHaveText('Rust is fine.');
    // The conversation goes on after the restart.
    provider.script = { chunks: ['Still ', 'here.'], pauseMs: 0 };
    await ask(second.page, 'and now');
    await expect(assistant(second.page).last()).toHaveText('Still here.');
    await expect(rows(second.page)).toHaveCount(1);
    await expectNoPolicyViolations(second.page);
  } finally {
    await second.app.close();
  }
});

test('New chat, delete and Delete all work, and the newest conversation is on top', async ({
  page,
  provider,
}) => {
  await configure(page, { address: provider.url });
  for (const message of ['one', 'two', 'three']) {
    provider.script = { chunks: [`answer to ${message}`], pauseMs: 0 };
    await page.locator('#chat-new').click();
    await ask(page, message);
    await expect(assistant(page)).toHaveText(`answer to ${message}`);
    await expect(rows(page)).toHaveCount(['one', 'two', 'three'].indexOf(message) + 1);
  }
  await expect(rows(page)).toHaveText(['three', 'two', 'one']);

  await page.locator('#chat-new').click();
  await expect(page.locator('#chat-empty')).toBeVisible();
  await expect(user(page)).toHaveCount(0);

  await rows(page).nth(1).getByRole('button').first().click();
  await expect(user(page)).toHaveText(['two']);
  await expect(assistant(page)).toHaveText('answer to two');

  await page.getByRole('button', { name: 'Delete conversation: two' }).click();
  await expect(rows(page)).toHaveText(['three', 'one']);
  // The conversation that was open is gone, so the page is a new chat.
  await expect(page.locator('#chat-empty')).toBeVisible();

  await page.locator('#chat-delete-all').click();
  await expect(page.getByRole('alertdialog', { name: 'Delete all conversations?' })).toBeVisible();
  await page.locator('#chat-delete-all-cancel').click();
  await expect(rows(page)).toHaveCount(2);
  await page.locator('#chat-delete-all').click();
  await page.locator('#chat-delete-all-confirm').click();
  await expect(rows(page)).toHaveCount(0);
  await expect(page.locator('#chat-delete-all')).toHaveCount(0);
});

test('a wrong key shows its reason within 15 seconds', async ({ page, provider }) => {
  await configure(page, { address: provider.url, key: 'sk-wrong' });
  await ask(page, 'hello');
  await expect(page.locator('#chat-failure')).toContainText('did not accept the API key', {
    timeout: reasonDeadlineMs,
  });
  await expect(page.locator('#chat-failure-settings')).toBeVisible();
});

test('an unknown model shows its reason within 15 seconds', async ({ page, provider }) => {
  await configure(page, { address: provider.url, model: 'no-such-model' });
  await ask(page, 'hello');
  await expect(page.locator('#chat-failure')).toContainText('no model with this name', {
    timeout: reasonDeadlineMs,
  });
});

test('an address with nothing behind it shows its reason within 15 seconds', async ({ page }) => {
  await configure(page, { address: `http://127.0.0.1:${await closedPort()}/v1` });
  await ask(page, 'hello');
  await expect(page.locator('#chat-failure')).toContainText('Nothing answered', {
    timeout: reasonDeadlineMs,
  });
});

test('leaving the page ends a running answer', async ({ page, provider }) => {
  provider.script = { chunks: ['Part ', 'never'], pauseMs: 0, holdAt: 1, forever: true };
  await configure(page, { address: provider.url });
  await ask(page, 'long answer please');
  await expect(assistant(page)).toContainText('Part');

  await page.getByRole('button', { name: 'Status', exact: true }).click();
  await expect.poll(() => provider.aborted(), { timeout: stopDeadlineMs }).toBe(1);
  await page.getByRole('button', { name: 'Chat', exact: true }).click();
  await expect(page.locator('#chat-failure')).toContainText('Stopped');
  await expect(assistant(page)).toHaveText('Part');
});

test('neither the key nor the message is in the agent log, the status answer or the output', async ({
  session,
  provider,
}) => {
  const secretMessage = 'private-question-5e91c7';
  provider.script = { chunks: ['An ', 'answer.'], pauseMs: 0 };
  const { app, page, output } = await openApp(session);
  const testOutput = captureTestOutput();
  const logged: string[] = [];
  page.on('console', (message) => logged.push(message.text()));
  try {
    await configure(page, { address: provider.url });
    await ask(page, secretMessage);
    await expect(assistant(page)).toHaveText('An answer.');
    // A refused answer, where an error could repeat the request.
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await page.getByLabel('API key', { exact: true }).fill('sk-wrong');
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(page.locator('#model-outcome')).toContainText('Saved');
    await page.getByRole('button', { name: 'Chat', exact: true }).click();
    await ask(page, `${secretMessage} again`);
    const failure = page.locator('#chat-failure');
    await expect(failure).toContainText('did not accept the API key');
    // Read while the thread and the refusal are on screen. The provider's refusal repeats the key
    // and the request, so a window that let it through would show it here.
    const window = await windowText(page);
    const failureText = await failure.innerText();
    const storage = await page.evaluate(() =>
      JSON.stringify({ ...localStorage, ...sessionStorage }),
    );
    expect(window).toContain(secretMessage);
    await page.getByRole('button', { name: 'Status', exact: true }).click();
    await expect(page.locator('#agent-model')).toHaveText(`model ${goodModel}`, {
      timeout: 10_000,
    });
    const places: Record<string, string> = {
      'the agent log': session.agentLog(),
      'the status answer': execFileSync(agentBinary, ['status'], { env: session.env }).toString(),
      'the console of the window': logged.join('\n'),
      'the output of the app': output(),
      'the output of the test': testOutput.text(),
    };
    for (const [name, text] of Object.entries(places)) {
      expect(text, name).not.toContain(goodKey);
      expect(text, name).not.toContain(secretMessage);
    }
    // The window shows the messages (it is the chat), but never a key or the provider's own text,
    // and it keeps no message in its storage.
    for (const text of [window, failureText]) {
      expect(text).not.toContain(goodKey);
      expect(text).not.toContain('sk-wrong');
      expect(text).not.toContain('bad key');
    }
    expect(storage).not.toContain(secretMessage);
    expect(session.agentLog()).toContain('chat: answer ended');
    await expectNoPolicyViolations(page);
  } finally {
    testOutput.stop();
    await app.close();
  }
});

test('Enter that confirms an input method does not send', async ({ page, provider }) => {
  await configure(page, { address: provider.url });
  await input(page).fill('こんにちは');
  const prevented = await input(page).evaluate((element) => {
    const event = new KeyboardEvent('keydown', {
      key: 'Enter',
      isComposing: true,
      bubbles: true,
      cancelable: true,
    });
    element.dispatchEvent(event);
    return event.defaultPrevented;
  });
  expect(prevented).toBe(false);
  await expect(input(page)).toHaveValue('こんにちは');
  expect(provider.seen).toHaveLength(0);
  await input(page).press('Enter');
  await expect(assistant(page)).toHaveCount(1);
  expect(provider.seen).toHaveLength(1);
});

test('a message typed while a conversation is being read stays in the box and is not sent', async ({
  page,
  provider,
  session,
}) => {
  provider.script = { chunks: ['Stored answer.'], pauseMs: 0 };
  await configure(page, { address: provider.url });
  await ask(page, 'stored question');
  await expect(assistant(page)).toHaveText('Stored answer.');
  await page.locator('#chat-new').click();

  // A stopped agent cannot answer the read, so the conversation stays "loading".
  session.agent.kill('SIGSTOP');
  try {
    await rows(page).first().getByRole('button').first().click();
    await expect(page.getByLabel('Loading the conversation')).toBeVisible();
    await input(page).fill('typed while loading');
    await input(page).press('Enter');
    await expect(input(page)).toHaveValue('typed while loading');
    expect(provider.seen).toHaveLength(1);
  } finally {
    session.agent.kill('SIGCONT');
  }
  await expect(turns(page)).toHaveText(['stored question', 'Stored answer.']);
  provider.script = { chunks: ['Second answer.'], pauseMs: 0 };
  await input(page).press('Enter');
  await expect(turns(page)).toHaveText([
    'stored question',
    'Stored answer.',
    'typed while loading',
    'Second answer.',
  ]);
});

test('a second conversation chosen while one is being read is ignored', async ({
  page,
  provider,
  session,
}) => {
  await configure(page, { address: provider.url });
  for (const message of ['older', 'newer']) {
    provider.script = { chunks: [`answer to ${message}`], pauseMs: 0 };
    await page.locator('#chat-new').click();
    await ask(page, message);
    await expect(assistant(page)).toHaveText(`answer to ${message}`);
  }
  await expect(rows(page)).toHaveText(['newer', 'older']);
  await page.locator('#chat-new').click();

  session.agent.kill('SIGSTOP');
  try {
    await rows(page).nth(1).getByRole('button').first().click();
    await expect(page.getByLabel('Loading the conversation')).toBeVisible();
    // A second read would be the other conversation; it must not start.
    await rows(page).nth(0).getByRole('button').first().click({ force: true });
  } finally {
    session.agent.kill('SIGCONT');
  }
  await expect(turns(page)).toHaveText(['older', 'answer to older']);
  await expect(rows(page).nth(1).getByRole('button').first()).toHaveAttribute(
    'aria-current',
    'true',
  );
});

test('an example question is sent by a click', async ({ page, provider }) => {
  provider.script = { chunks: ['Rust is ', 'fine.'], pauseMs: 0 };
  await configure(page, { address: provider.url });
  await page.getByRole('button', { name: 'I mostly write Rust' }).click();
  await expect(user(page)).toHaveText(['I mostly write Rust']);
  await expect(assistant(page)).toHaveText('Rust is fine.');
  await expect(rows(page)).toHaveCount(1);
  await expect(page.locator('#chat-empty')).toHaveCount(0);
});

test('New chat, open, delete and Delete all are off while an answer runs', async ({
  page,
  provider,
}) => {
  provider.script = { chunks: ['First answer.'], pauseMs: 0 };
  await configure(page, { address: provider.url });
  await ask(page, 'first');
  await expect(assistant(page)).toHaveText('First answer.');
  await expect(rows(page)).toHaveCount(1);

  provider.script = { chunks: ['Part ', 'never'], pauseMs: 0, holdAt: 1, forever: true };
  await ask(page, 'second');
  await expect(assistant(page).last()).toContainText('Part');
  const controls = [
    page.locator('#chat-new'),
    page.locator('#chat-delete-all'),
    rows(page).first().getByRole('button').first(),
    rows(page).first().getByRole('button').last(),
  ];
  for (const control of controls) {
    await expect(control).toBeDisabled();
  }

  await page.locator('#chat-stop-button').click();
  await expect(page.locator('#chat-failure')).toContainText('Stopped');
  for (const control of controls) {
    await expect(control).toBeEnabled();
  }
});

test('screenshots of a conversation in the light and the dark theme', async ({
  page,
  provider,
}) => {
  await configure(page, { address: provider.url });
  provider.script = { chunks: ['Rust is a good choice for ', 'a local agent.'], pauseMs: 0 };
  await ask(page, 'I mostly write Rust');
  await expect(assistant(page)).toHaveText('Rust is a good choice for a local agent.');
  provider.script = {
    chunks: [
      'Here is what I know so far:\n',
      '- you write Rust\n',
      '- you like small, local tools\n',
    ],
    pauseMs: 0,
  };
  await page.locator('#chat-new').click();
  await ask(page, 'What do you know about me?');
  await expect(assistant(page)).toContainText('small, local tools');
  await expect(rows(page)).toHaveCount(2);
  await screenshotBothThemes(page, 'chat-streamed');
});
