import { useEffect, useState } from 'react';
import { dayLabel, mondayOf, weekBounds } from './dashboard-period';
import type { SettingsResult } from '../agent-settings';
import type { DashboardResult, Whose } from '../dashboard-protocol';

/** What the page knows of the week it shows. */
export type DashboardState =
  | { readonly kind: 'reading' }
  | { readonly kind: 'failed'; readonly reason: string }
  | {
      readonly kind: 'ready';
      readonly totalMinutes: number;
      readonly sessionCount: number;
      /** The minutes of the day the page is on. */
      readonly dayMinutes: number;
      /** Whether the agent collects the user's work. */
      readonly collecting: boolean;
    };

/** The answer for a whole week: what the page keeps while the day moves inside it. */
type Week =
  | { readonly kind: 'failed'; readonly reason: string }
  | {
      readonly kind: 'ready';
      readonly totalMinutes: number;
      readonly sessionCount: number;
      readonly minutes: ReadonlyMap<string, number>;
      readonly collecting: boolean;
    };

/** How long the page waits before it asks again while the agent reads the history. */
const retryMs = 2000;

function weekOf(numbers: DashboardResult, settings: SettingsResult): Week {
  if (!numbers.ok) {
    return { kind: 'failed', reason: numbers.error };
  }
  if (!settings.ok) {
    // Without the settings the page cannot say whether collection is on.
    return { kind: 'failed', reason: settings.error };
  }
  return {
    kind: 'ready',
    totalMinutes: numbers.totalMinutes,
    sessionCount: numbers.sessionCount,
    minutes: new Map(numbers.days.map((day) => [day.label, day.minutes])),
    collecting: settings.collection.enabled,
  };
}

/**
 * The numbers of the week of `anchor` for the time `whose`, and whether collection is on. The
 * page reads again, and says so, each time the week or the choice changes. The days of a week
 * come in one answer, so a move to another day of the week reads nothing again.
 */
export function useDashboard(anchor: Date, whose: Whose): DashboardState {
  const key = `${dayLabel(mondayOf(anchor))}|${whose}`;
  const [loaded, setLoaded] = useState<{ key: string; week: Week } | null>(null);
  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const read = (): void => {
      void Promise.all([
        window.constructAgent.getDashboardNumbers({ days: weekBounds(anchor), sessions: whose }),
        window.constructAgent.getSettings(),
      ]).then(([numbers, settings]) => {
        if (cancelled) {
          return;
        }
        // The agent reads the history first: the page says so, and asks again.
        if (!numbers.ok && numbers.reading === true) {
          timer = setTimeout(read, retryMs);
          return;
        }
        setLoaded({ key, week: weekOf(numbers, settings) });
      });
    };
    read();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [key]);
  if (loaded?.key !== key) {
    return { kind: 'reading' };
  }
  const { week } = loaded;
  if (week.kind === 'failed') {
    return week;
  }
  // The agent answers for every day asked, empty days included, and the week asks for all seven.
  const dayMinutes = week.minutes.get(dayLabel(anchor)) ?? 0;
  return {
    kind: 'ready',
    totalMinutes: week.totalMinutes,
    sessionCount: week.sessionCount,
    dayMinutes,
    collecting: week.collecting,
  };
}
