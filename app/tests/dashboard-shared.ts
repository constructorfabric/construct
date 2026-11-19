import type { Page } from '@playwright/test';
import type { Whose } from '../src/dashboard-protocol';

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

/** A widget of a stand-in dashboard document, as the agent writes it. */
export interface StandInWidget {
  readonly id: string;
  readonly kind: string;
  readonly group_by?: string;
  readonly period?: string;
  readonly sessions?: Whose;
  readonly source?: 'ai_sessions' | 'on_screen';
  readonly position: { column: number; row: number; width: number; height: number };
}

/** A widget at `[column, row, width, height]`. */
export function widget(
  id: string,
  kind: string,
  at: [number, number, number, number],
  more: Partial<StandInWidget> = {},
): StandInWidget {
  return {
    id,
    kind,
    position: { column: at[0], row: at[1], width: at[2], height: at[3] },
    ...more,
  };
}

/** The dashboard document the stand-in agent answers with, around `widgets`. */
export function gridPlaces(widgets: readonly StandInWidget[], sessions: Whose = 'own') {
  return {
    type: 'a dashboard type',
    title: 'Activity',
    payload: {
      period: 'this_week',
      sessions,
      widgets,
      hidden_projects: [] as string[],
      project_names: {} as Record<string, string>,
    },
  };
}
