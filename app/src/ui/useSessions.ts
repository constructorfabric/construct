import { useCallback, useEffect, useRef, useState } from 'react';
import type { Rows, SessionsInput } from '../dashboard-protocol';
import { lostAgent } from './lost-agent';

/** Sessions on a page of the drill-down. */
export const pageSize = 20;

/** What the drill-down asks the agent, but for the page: the same for every page. */
type Without<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;
export type SessionsSource = Without<SessionsInput, 'offset' | 'limit'>;

export interface SessionsList {
  /**
   * The rows loaded so far, the earliest first: sessions, or the blocks of the time on screen,
   * as the agent answered; `null` until the first page is in.
   */
  readonly list: Rows | null;
  /** All the sessions of the group, every page together; `null` until the first page is in. */
  readonly total: number | null;
  readonly loading: boolean;
  readonly error: string | null;
  /** Whether a page is left to load. */
  readonly hasMore: boolean;
  readonly more: () => void;
}

/** The sessions of one group, a page at a time, as the agent counted them. */
export function useSessions(source: SessionsSource): SessionsList {
  const [list, setList] = useState<Rows | null>(null);
  const [total, setTotal] = useState<number | null>(null);
  const [next, setNext] = useState<number | null>(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The page being asked for: a second click on More while it loads asks for nothing.
  const asking = useRef(false);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const load = useCallback(
    (offset: number) => {
      if (asking.current) {
        return;
      }
      asking.current = true;
      setLoading(true);
      setError(null);
      window.constructAgent
        .getDashboardSessions({ ...source, offset, limit: pageSize })
        .then(
          (result) => {
            if (!alive.current) {
              return;
            }
            if (result.ok) {
              if (result.kind === 'blocks') {
                setList((current) => ({
                  kind: 'blocks',
                  rows: [...(current?.kind === 'blocks' ? current.rows : []), ...result.rows],
                }));
              } else {
                setList((current) => ({
                  kind: 'sessions',
                  rows: [...(current?.kind === 'sessions' ? current.rows : []), ...result.rows],
                }));
              }
              setTotal(result.total);
              setNext(result.nextOffset);
            } else {
              setError(result.error);
            }
          },
          (failure: unknown) => {
            if (alive.current) {
              setError(lostAgent(failure));
            }
          },
        )
        .finally(() => {
          asking.current = false;
          if (alive.current) {
            setLoading(false);
          }
        });
    },
    // The source of one drill-down never changes: the dialog is drawn anew for another group.
    [],
  );
  useEffect(() => {
    load(0);
  }, [load]);
  return {
    list,
    total,
    loading,
    error,
    hasMore: next !== null && total !== null,
    more: () => {
      if (next !== null) {
        load(next);
      }
    },
  };
}
