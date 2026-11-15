import type { Page } from '@playwright/test';
import { expect, test as base } from './fixtures';
import { goodKey, goodModel, providerFixture } from './provider';
import type { Provider } from './provider';

/** The window's test with a fake `provider` for each test. */
export const test = base.extend<{ provider: Provider }>(providerFixture);

/** Saves a model on the Settings page and goes back to the Chat page. */
export async function configure(
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

export const input = (page: Page) => page.locator('#chat-input');
export const assistant = (page: Page) => page.locator('[data-role=assistant]');

export async function ask(page: Page, message: string): Promise<void> {
  await input(page).fill(message);
  await input(page).press('Enter');
}
