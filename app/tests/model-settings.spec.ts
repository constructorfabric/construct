import { execFileSync } from 'node:child_process';
import type { Page } from '@playwright/test';
import {
  agentBinary,
  captureTestOutput,
  checkInApp,
  expectNoPolicyViolations,
  expect,
  openApp,
  openSettings,
  restartAgent,
  test as base,
  windowText,
} from './fixtures';
import { closedPort, goodKey, goodModel, providerFixture } from './provider';
import type { Provider } from './provider';
import { screenshotBothThemes } from './screenshots';

const unreachableDeadlineMs = 15_000;

const test = base.extend<{ provider: Provider }>(providerFixture);

async function fill(page: Page, fields: { address?: string; model?: string; key?: string }) {
  if (fields.address !== undefined) {
    await page.getByLabel('Address').fill(fields.address);
  }
  if (fields.model !== undefined) {
    await page.getByLabel('Model name').fill(fields.model);
  }
  if (fields.key !== undefined) {
    await page.getByLabel('API key', { exact: true }).fill(fields.key);
  }
}

const outcome = (page: Page) => page.locator('#model-outcome');

test('the model section starts with the defaults, and the key field is empty', async ({ page }) => {
  await openSettings(page, 'Model');

  await expect(page.getByLabel('Address')).toHaveValue('https://api.openai.com/v1');
  await expect(page.getByLabel('Model name')).toHaveValue('gpt-6-luna');
  const key = page.getByLabel('API key', { exact: true });
  await expect(key).toHaveValue('');
  await expect(key).toHaveAttribute('type', 'password');
  await expect(page.getByRole('button', { name: 'Remove key' })).toHaveCount(0);
});

test('Test shows a spinner and then Works with the answer of the model', async ({
  page,
  provider,
}) => {
  provider.delayMs = 3000;
  await openSettings(page, 'Model');
  await fill(page, { address: provider.url, model: goodModel, key: goodKey });

  await page.getByRole('button', { name: 'Test' }).click();
  // The kit marks a loading button with aria-busy and shows its spinner in it.
  await expect(page.locator('#model-test')).toHaveAttribute('aria-busy', 'true');
  await expect(page.locator('#model-save')).toBeDisabled();

  await expect(outcome(page)).toContainText('Works');
  await expect(outcome(page)).toContainText('pong');
  expect(provider.seen.at(-1)).toEqual({ authorization: `Bearer ${goodKey}`, model: goodModel });
});

test('a wrong key shows key refused', async ({ page, provider }) => {
  await openSettings(page, 'Model');
  await fill(page, { address: provider.url, model: goodModel, key: 'sk-wrong' });

  await page.getByRole('button', { name: 'Test' }).click();

  await expect(outcome(page)).toContainText('key refused');
});

test('an unknown model shows model not found', async ({ page, provider }) => {
  await openSettings(page, 'Model');
  await fill(page, { address: provider.url, model: 'no-such-model', key: goodKey });

  await page.getByRole('button', { name: 'Test' }).click();

  await expect(outcome(page)).toContainText('model not found');
});

test('an address with nothing behind it shows unreachable within 15 seconds', async ({ page }) => {
  await openSettings(page, 'Model');
  await fill(page, {
    address: `http://127.0.0.1:${await closedPort()}/v1`,
    model: goodModel,
    key: goodKey,
  });

  await page.getByRole('button', { name: 'Test' }).click();

  await expect(outcome(page)).toContainText('unreachable', { timeout: unreachableDeadlineMs });
});

test('a non-local http address is refused with a clear message, by Test and by Save', async ({
  page,
}) => {
  await openSettings(page, 'Model');
  await fill(page, { address: 'http://example.com/v1', model: goodModel, key: goodKey });

  await page.getByRole('button', { name: 'Test' }).click();
  await expect(outcome(page)).toContainText('base_url must be an https address');

  await page.getByRole('button', { name: 'Save' }).click();
  await expect(outcome(page)).toContainText('base_url must be an https address');
  await expect(outcome(page)).not.toContainText(goodKey);
});

test('Save keeps the saved key when the key field is untouched, and Remove key drops it', async ({
  page,
  provider,
}) => {
  await openSettings(page, 'Model');
  await fill(page, { address: provider.url, model: goodModel, key: goodKey });
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(outcome(page)).toContainText('Saved');

  // Only the model name changes; the key field is empty, so the agent keeps the key.
  await fill(page, { model: goodModel });
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(outcome(page)).toContainText('Saved');
  await page.getByRole('button', { name: 'Test' }).click();
  await expect(outcome(page)).toContainText('Works');
  expect(provider.seen.at(-1)?.authorization).toBe(`Bearer ${goodKey}`);

  await page.getByRole('button', { name: 'Remove key' }).click();
  await expect(outcome(page)).toContainText('The key is removed');
  await expect(page.getByRole('button', { name: 'Remove key' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Test' }).click();
  await expect(outcome(page)).toContainText('key refused');
});

test('Save with a blank Address and Model name shows what is required and sends nothing', async ({
  page,
}) => {
  await openSettings(page, 'Model');
  await fill(page, { address: '', model: '', key: goodKey });
  await expect(page.getByText('This field is required')).toHaveCount(2);

  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('This field is required')).toHaveCount(2);
  await expect(outcome(page)).toHaveCount(0);

  // The agent was never asked to save: it still has no model.
  await page.getByRole('button', { name: 'Status', exact: true }).click();
  await expect(page.locator('#agent-model')).toHaveText('model not set');
});

test('another address is not sent the saved key, and typing the key again allows it', async ({
  page,
  provider,
}) => {
  await openSettings(page, 'Model');
  await fill(page, { address: provider.url, model: goodModel, key: goodKey });
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(outcome(page)).toContainText('Saved');
  const requests = provider.seen.length;

  // The key field is empty, so the only key is the saved one, which belongs to the saved address.
  await fill(page, { address: `${provider.url}/` });
  await page.getByRole('button', { name: 'Test' }).click();
  await expect(outcome(page)).toContainText('type the key again');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(outcome(page)).toContainText('type the key again');
  expect(provider.seen).toHaveLength(requests);

  await fill(page, { key: goodKey });
  await page.getByRole('button', { name: 'Test' }).click();
  await expect(outcome(page)).toContainText('Works');
});

test('the Status page shows model not set, then the saved model name', async ({
  page,
  provider,
}) => {
  await page.getByRole('button', { name: 'Status', exact: true }).click();
  await expect(page.locator('#agent-model')).toHaveText('model not set', { timeout: 10_000 });

  await openSettings(page, 'Model');
  await fill(page, { address: provider.url, model: goodModel, key: goodKey });
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(outcome(page)).toContainText('Saved');

  await page.getByRole('button', { name: 'Status', exact: true }).click();
  await expect(page.locator('#agent-model')).toHaveText(`model ${goodModel}`, {
    timeout: 10_000,
  });
});

test('after Save, closing the app and the agent and starting both again keeps the model and the key', async ({
  session,
  provider,
}) => {
  await checkInApp(session, async (page) => {
    await openSettings(page, 'Model');
    await fill(page, { address: provider.url, model: goodModel, key: goodKey });
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(outcome(page)).toContainText('Saved');
    const key = page.getByLabel('API key', { exact: true });
    await expect(key).toHaveValue('');
    await expect(key).toHaveAttribute('placeholder', '••••••••');
  });
  await restartAgent(session);

  await checkInApp(session, async (page) => {
    await openSettings(page, 'Model');
    await expect(page.getByLabel('Model name')).toHaveValue(goodModel);
    await expect(page.getByLabel('Address')).toHaveValue(provider.url);
    const key = page.getByLabel('API key', { exact: true });
    await expect(key).toHaveValue('');
    await expect(key).toHaveAttribute('placeholder', '••••••••');

    // The saved key still works, though the window never had it.
    await page.getByRole('button', { name: 'Test' }).click();
    await expect(outcome(page)).toContainText('Works');
  });
});

test('the key is nowhere but in the settings file and the request to the provider', async ({
  session,
  provider,
}) => {
  const { app, page, output } = await openApp(session);
  const testOutput = captureTestOutput();
  const logged: string[] = [];
  page.on('console', (message) => logged.push(message.text()));
  try {
    await openSettings(page, 'Model');
    await fill(page, { address: provider.url, model: goodModel, key: goodKey });
    await page.getByRole('button', { name: 'Test' }).click();
    await expect(outcome(page)).toContainText('Works');
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(outcome(page)).toContainText('Saved');
    // A refused save, which is where an error message could repeat the request.
    await fill(page, { address: 'http://example.com/v1', key: goodKey });
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(outcome(page)).toContainText('Not done');
    await fill(page, { address: provider.url });
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(outcome(page)).toContainText('Saved');
    await page.getByRole('button', { name: 'Status', exact: true }).click();
    await expect(page.locator('#agent-model')).toHaveText(`model ${goodModel}`, {
      timeout: 10_000,
    });
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await expect(page.getByLabel('API key', { exact: true })).toHaveValue('');

    // The key did reach the provider, so an empty search below means something.
    expect(provider.seen.some((request) => request.authorization === `Bearer ${goodKey}`)).toBe(
      true,
    );
    const places: Record<string, string> = {
      'the agent log': session.agentLog(),
      'the status answer': execFileSync(agentBinary, ['status'], { env: session.env }).toString(),
      'the window text': await windowText(page),
      'the console of the window': logged.join('\n'),
      'the output of the app': output(),
      'the output of the test': testOutput.text(),
    };
    for (const [name, text] of Object.entries(places)) {
      expect(text, name).not.toContain(goodKey);
    }
    await expectNoPolicyViolations(page);
  } finally {
    testOutput.stop();
    await app.close();
  }
});

test('the page with a saved key looks right in both themes', async ({ page }) => {
  await openSettings(page, 'Model');
  await screenshotBothThemes(page, 'settings-defaults');

  await fill(page, {
    address: 'https://api.example.com/v1',
    model: 'example-model',
    key: 'example-key',
  });
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(outcome(page)).toContainText('Saved');
  await expect(page.getByRole('button', { name: 'Remove key' })).toBeVisible();
  await screenshotBothThemes(page, 'settings-saved');
});
