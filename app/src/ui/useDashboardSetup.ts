import { addDays, startOfDay } from 'date-fns';
import { useCallback, useEffect, useState } from 'react';
import { isRunning, setupDays } from '../dashboard-setup-protocol';
import type { SetupStatus } from '../dashboard-setup-protocol';
import { dayBounds } from './dashboard-period';

/** How often the page asks how far a run is. */
const pollMs = 1000;

/** What the Settings page knows of the dashboard setup. */
export type SetupView =
  | { readonly kind: 'loading' }
  | { readonly kind: 'unavailable'; readonly reason: string }
  | {
      readonly kind: 'ready';
      readonly status: SetupStatus;
      /** How many sessions the period holds, when the agent could count them. */
      readonly sessionsInPeriod: number | null;
      /** Why the agent did not start the run the person asked for. */
      readonly refused: string | null;
      /** Why the agent could not be asked how the run goes; a run that goes is still shown. */
      readonly unreachable: string | null;
      /** A press is on its way to the agent. */
      readonly pending: 'start' | 'cancel' | null;
    };

export interface DashboardSetup {
  readonly view: SetupView;
  readonly start: () => void;
  readonly cancel: () => void;
}

/** The days of the period the setup reads, ending today. */
function periodDays(today: Date): ReturnType<typeof dayBounds>[] {
  return Array.from({ length: setupDays }, (_, index) =>
    dayBounds(addDays(startOfDay(today), index - setupDays + 1)),
  );
}

/**
 * The setup of the dashboard: the state of the run as the agent keeps it, asked again each second
 * while a run goes, so a window opened in the middle of a run shows it. Before a run the page
 * also counts the sessions of the period.
 */
export function useDashboardSetup(): DashboardSetup {
  const [view, setView] = useState<SetupView>({ kind: 'loading' });
  const running = view.kind === 'ready' && isRunning(view.status.state);

  const apply = useCallback((status: SetupStatus): void => {
    setView((current) =>
      current.kind === 'ready'
        ? { ...current, status, pending: null }
        : {
            kind: 'ready',
            status,
            sessionsInPeriod: null,
            refused: null,
            unreachable: null,
            pending: null,
          },
    );
  }, []);

  useEffect(() => {
    let cancelled = false;
    void window.constructAgent.getDashboardSetup().then((result) => {
      if (cancelled) {
        return;
      }
      if (!result.ok) {
        setView({ kind: 'unavailable', reason: result.error });
        return;
      }
      apply(result.status);
      void window.constructAgent
        .getDashboardNumbers({ days: periodDays(new Date()), sessions: 'all' })
        .then((numbers) => {
          if (!cancelled && numbers.ok) {
            setView((current) =>
              current.kind === 'ready'
                ? { ...current, sessionsInPeriod: numbers.sessionCount }
                : current,
            );
          }
        });
    });
    return () => {
      cancelled = true;
    };
  }, [apply]);

  useEffect(() => {
    if (!running) {
      return undefined;
    }
    let cancelled = false;
    const timer = setInterval(() => {
      void window.constructAgent.getDashboardSetup().then((result) => {
        if (cancelled) {
          return;
        }
        setView((current) => {
          if (current.kind !== 'ready') {
            return current;
          }
          // The run goes on in the agent: the page keeps showing it and says it cannot be asked.
          if (!result.ok) {
            return { ...current, unreachable: result.error };
          }
          return {
            ...current,
            status: result.status,
            unreachable: null,
            // A cancel is over once the run is.
            pending: isRunning(result.status.state) ? current.pending : null,
          };
        });
      });
    }, pollMs);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [running]);

  const start = useCallback((): void => {
    setView((current) =>
      current.kind === 'ready' ? { ...current, pending: 'start', refused: null } : current,
    );
    void window.constructAgent.startDashboardSetup().then((result) => {
      if (result.ok) {
        apply(result.status);
      } else {
        setView((current) =>
          current.kind === 'ready' ? { ...current, pending: null, refused: result.error } : current,
        );
      }
    });
  }, [apply]);

  const cancel = useCallback((): void => {
    setView((current) => (current.kind === 'ready' ? { ...current, pending: 'cancel' } : current));
    void window.constructAgent.cancelDashboardSetup().then((result) => {
      setView((current) => {
        if (current.kind !== 'ready') {
          return current;
        }
        // The run ends a moment after the press: the Cancel button waits until the agent says so. A
        // run that ended before the press has nothing to cancel, and the next answer says how it ended.
        return result.ok
          ? {
              ...current,
              status: result.status,
              unreachable: null,
              pending: isRunning(result.status.state) ? 'cancel' : null,
            }
          : { ...current, pending: null, unreachable: result.error };
      });
    });
  }, []);

  return { view, start, cancel };
}
