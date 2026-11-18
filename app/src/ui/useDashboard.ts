import { useEffect, useState } from 'react';
import { dayLabel, mondayOf, weekBounds } from './dashboard-period';
import type { Whose } from '../dashboard-protocol';

/** What the page knows of the week it shows. */
export type DashboardState =
  | { readonly kind: 'reading' }
  | { readonly kind: 'failed'; readonly reason: string }
  | {
      readonly kind: 'ready';
      readonly totalMinutes: number;
      readonly sessions: number;
      /** The minutes of a day of the week that was read. */
      readonly minutesOn: (date: Date) => number;
      /** Whether the agent collects the user's work. */
      readonly collecting: boolean;
    };

const incomplete = 'the agent did not answer for every day of the week';

/**
 * The numbers of the week of `anchor` for the time `whose`, and whether collection is on. The
 * page reads again, and says so, each time the week, the day or the choice changes.
 */
export function useDashboard(anchor: Date, whose: Whose): DashboardState {
  // The days of a week come in one answer, so a move within the week reads nothing again.
  const key = `${dayLabel(mondayOf(anchor))}|${whose}`;
  const [loaded, setLoaded] = useState<{ key: string; state: DashboardState } | null>(null);
  useEffect(() => {
    let cancelled = false;
    void Promise.all([
      window.constructAgent.getDashboardNumbers({ days: weekBounds(anchor), sessions: whose }),
      window.constructAgent.getSettings(),
    ]).then(([numbers, settings]) => {
      if (cancelled) {
        return;
      }
      const week = weekBounds(anchor);
      const minutes = new Map(numbers.ok ? numbers.days.map((d) => [d.label, d.minutes]) : []);
      let state: DashboardState;
      if (!numbers.ok) {
        state = { kind: 'failed', reason: numbers.error };
      } else if (!settings.ok) {
        // Without the settings the page cannot say whether collection is on.
        state = { kind: 'failed', reason: settings.error };
      } else if (week.some((day) => !minutes.has(day.label))) {
        state = { kind: 'failed', reason: incomplete };
      } else {
        state = {
          kind: 'ready',
          totalMinutes: numbers.totalMinutes,
          sessions: numbers.sessions,
          minutesOn: (date) => {
            const found = minutes.get(dayLabel(date));
            if (found === undefined) {
              throw new Error('a day outside the week that was read');
            }
            return found;
          },
          collecting: settings.collection.enabled,
        };
      }
      setLoaded({ key, state });
    });
    return () => {
      cancelled = true;
    };
  }, [key]);
  return loaded?.key === key ? loaded.state : { kind: 'reading' };
}
