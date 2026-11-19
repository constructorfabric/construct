import type { Page } from '@playwright/test';

/** The window's clock stands at Wednesday 18 November 2026, so the week is a known one. */
export const now = new Date(2026, 10, 18, 10, 0, 0);

/** What the agent answers to `settings-get`. */
export function settingsReply(
  collecting: boolean,
  screen: { readonly supported: boolean; readonly on: boolean } = { supported: false, on: false },
): string {
  return JSON.stringify({
    model: { base_url: 'https://example.invalid/v1', model: 'example-model', api_key_set: false },
    collection: {
      enabled: collecting,
      claude_code: collecting,
      codex: collecting,
      screen: screen.on,
    },
    supports: { screen: screen.supported },
    notice: null,
  });
}

/** The text of the first cell of each row of a table, in the order shown. */
export async function firstCells(page: Page, id: string): Promise<string[]> {
  return page.locator(`#table-${id} tbody tr td:first-child`).allTextContents();
}
