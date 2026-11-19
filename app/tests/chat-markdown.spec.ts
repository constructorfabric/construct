import { ask, assistant, configure, test } from './chat-helpers';
import { expect, recordOpenedLinks } from './fixtures';
import { screenshotBothThemes } from './screenshots';

const link = 'https://example.com/docs';

// Everything a Markdown answer is made of, and the two things that must never run.
const answer = [
  '# Weekly plan',
  '',
  'Here is the **plan** for the week, with `inline code` and a [guide](' + link + ').',
  '',
  '- Write the report',
  '- Review the notes',
  '',
  '| Day | Task |',
  '| --- | --- |',
  '| Monday | Draft |',
  '| Tuesday | Review |',
  '',
  '> Keep it short.',
  '',
  '```rust',
  'fn main() {',
  '    println!("hello");',
  '}',
  '```',
  '',
  '<script>window.scriptRan = true</script>',
  '',
  'Done <img src="https://example.com/x.png" onerror="window.imageFailed = true"> and ![chart](https://example.com/chart.png).',
].join('\n');

const code = 'fn main() {\n    println!("hello");\n}';

test('an answer is shown as Markdown, and the HTML in it runs nothing', async ({
  page,
  provider,
}) => {
  provider.script = { chunks: [answer], pauseMs: 0 };
  await configure(page, { address: provider.url });
  await ask(page, 'plan');

  const reply = assistant(page);
  await expect(reply.getByRole('heading', { name: 'Weekly plan' })).toBeVisible();
  await expect(reply.getByRole('listitem')).toHaveText(['Write the report', 'Review the notes']);
  await expect(reply.getByRole('table')).toBeVisible();
  await expect(reply.getByRole('cell')).toHaveText(['Monday', 'Draft', 'Tuesday', 'Review']);
  await expect(reply.locator('blockquote')).toHaveText('Keep it short.');
  await expect(reply.locator('pre')).toHaveText(code);
  // The code is highlighted with classes, not style attributes (the window would refuse those):
  // the keyword `fn` has a colour of its own.
  await expect(reply.locator('pre span[style]')).toHaveCount(0);
  const colours = await reply.locator('pre').evaluate((pre) => {
    const keyword = [...pre.querySelectorAll('span')].find((span) => span.textContent === 'fn');
    return [keyword && getComputedStyle(keyword).color, getComputedStyle(pre).color];
  });
  expect(colours[0]).toBeTruthy();
  expect(colours[0]).not.toBe(colours[1]);
  await expect(reply.getByRole('link', { name: 'guide' })).toHaveAttribute('href', link);

  // The two tags are text, and nothing of them became an element or ran.
  await expect(reply).toContainText('<script>window.scriptRan = true</script>');
  await expect(reply.locator('script')).toHaveCount(0);
  await expect(reply.locator('img')).toHaveCount(0);
  expect(
    await page.evaluate(() => [
      Reflect.get(window, 'scriptRan'),
      Reflect.get(window, 'imageFailed'),
    ]),
  ).toEqual([undefined, undefined]);
  // An image of the answer is its description and its address, as a link.
  await expect(reply.getByRole('link', { name: 'https://example.com/chart.png' })).toBeVisible();
});

test('a link opens in the system browser and the window stays where it is', async ({
  app,
  page,
  provider,
}) => {
  const opened = await recordOpenedLinks(app);
  provider.script = {
    chunks: [
      `[guide](${link}) [local](file:///etc/hosts) [script](javascript:window.scriptRan=true)`,
    ],
    pauseMs: 0,
  };
  await configure(page, { address: provider.url });
  await ask(page, 'plan');

  // The window is asked to navigate only if the click was not taken: the app's own handler
  // would deny it, but the click must not get that far.
  await app.evaluate(({ BrowserWindow }) => {
    const attempts: string[] = [];
    BrowserWindow.getAllWindows()[0]?.webContents.on('will-navigate', (_event, url) => {
      attempts.push(url);
    });
    Object.assign(globalThis, { attempts });
  });
  await assistant(page).getByRole('link', { name: 'guide' }).click();
  await expect.poll(opened).toEqual([link]);
  expect(await app.evaluate(() => Reflect.get(globalThis, 'attempts') as string[])).toEqual([]);
  expect(app.windows()).toHaveLength(1);

  // An address that is not a web address is plain text, with nothing to click.
  await expect(assistant(page).getByRole('link')).toHaveCount(1);
  await expect(assistant(page)).toContainText('local');
  await expect(assistant(page)).toContainText('script');
  expect(await opened()).toEqual([link]);
});

test('the main process opens nothing but a web address, whoever asks', async ({ app, page }) => {
  const opened = await recordOpenedLinks(app);
  const results = await page.evaluate(() =>
    Promise.all(
      ['file:///etc/hosts', 'javascript:window.scriptRan=true', 'https://example.com/'].map((url) =>
        window.constructAgent.openLink(url),
      ),
    ),
  );
  expect(results).toEqual([false, false, true]);
  expect(await opened()).toEqual(['https://example.com/']);
});

test('a linked image opens its link once, and shows the address as text inside it', async ({
  app,
  page,
  provider,
}) => {
  const opened = await recordOpenedLinks(app);
  provider.script = {
    chunks: ['[![badge](https://example.com/badge.svg)](https://example.com/target)'],
    pauseMs: 0,
  };
  await configure(page, { address: provider.url });
  await ask(page, 'plan');

  const links = assistant(page).getByRole('link');
  await expect(links).toHaveCount(1);
  await expect(links).toContainText('badge https://example.com/badge.svg');
  await expect(assistant(page).locator('a a')).toHaveCount(0);
  await links.click();
  await expect.poll(opened).toEqual(['https://example.com/target']);
});

test('Copy puts the code of a block in the clipboard', async ({ app, page, provider }) => {
  provider.script = { chunks: [answer], pauseMs: 0 };
  await configure(page, { address: provider.url });
  await ask(page, 'plan');

  await assistant(page).getByRole('button', { name: 'Copy' }).click();
  await expect(assistant(page).getByRole('button', { name: 'Copied' })).toBeVisible();
  expect(await app.evaluate(({ clipboard }) => clipboard.readText())).toBe(code);
});

test('an answer that stops in an open code fence is shown and throws nothing', async ({
  page,
  provider,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('console', (message) => {
    if (message.type() === 'error') {
      errors.push(message.text());
    }
  });
  provider.script = {
    chunks: ['Here is the code:\n\n```rust\nfn main() {\n', '    println!("hi");\n'],
    pauseMs: 0,
    holdAt: 1,
    forever: true,
  };
  await configure(page, { address: provider.url });
  await ask(page, 'plan');

  // The fence is still open: what has come is already a code block.
  await expect(assistant(page).locator('pre')).toContainText('fn main() {');
  await expect(assistant(page)).toContainText('Here is the code:');
  await expect(page.locator('#chat-stop-button')).toBeVisible();
  expect(errors).toEqual([]);
});

test('a code block fills in while the answer arrives, and the block stays one element', async ({
  page,
  provider,
}) => {
  provider.script = {
    chunks: ['```\nline one\n', 'line two\n', 'line three\n```\n\nThe end.'],
    pauseMs: 150,
  };
  await configure(page, { address: provider.url });
  await ask(page, 'plan');

  await expect(assistant(page).locator('pre')).toContainText('line one');
  const block = await assistant(page).locator('pre').elementHandle();
  await expect(assistant(page)).toContainText('The end.');
  await expect(assistant(page).locator('pre')).toHaveText('line one\nline two\nline three');
  // The same element all along: the block was updated, not replaced.
  expect(await block?.evaluate((element) => element.isConnected)).toBe(true);
});

test('screenshots of an answer in the light and the dark theme', async ({
  app,
  page,
  provider,
}) => {
  // Tall enough for the whole answer, with its heading and its last line.
  await app.evaluate(({ BrowserWindow }) => {
    BrowserWindow.getAllWindows()[0]?.setSize(960, 960);
  });
  provider.script = { chunks: [answer], pauseMs: 0 };
  await configure(page, { address: provider.url });
  await ask(page, 'plan');
  await expect(assistant(page).getByRole('link', { name: 'guide' })).toBeVisible();
  await screenshotBothThemes(page, 'chat-markdown');
});
