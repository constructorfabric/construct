import { useCallback, useEffect, useRef, useState } from 'react';
import type { MemoryFact } from '../memory-protocol';
import { lostAgent as message } from './lost-agent';

/** How often the open page asks for the profile's version. */
const versionPollMs = 3000;

/** What the page knows of the person's facts. */
export type MemoryState =
  | { readonly kind: 'reading' }
  | { readonly kind: 'failed'; readonly reason: string }
  | {
      readonly kind: 'ready';
      readonly facts: readonly MemoryFact[];
      /** Whether the agent collects the user's work; `null` when the settings could not say. */
      readonly collecting: boolean | null;
    };

/** What the last delete came to; it stays until the next one. */
export type DeleteNotice =
  | { readonly kind: 'deleted' }
  | { readonly kind: 'failed'; readonly fact: string; readonly reason: string };

export interface Memory {
  readonly state: MemoryState;
  readonly notice: DeleteNotice | null;
  /** Whether a delete is waiting for the gear; one at a time, so the notice is always its own. */
  readonly deleting: boolean;
  /**
   * Deletes a fact; its row leaves the page only once the gear has said it is gone. `name` is
   * what the notice calls the fact.
   */
  readonly remove: (factId: string, name: string) => Promise<void>;
}

/**
 * The facts of the person. While the page is open it asks for the profile's version now and then
 * (one fact per call) and reads the list again when the version is not the one it has, so a new
 * fact appears by itself. After a failure the next look reads the list again.
 *
 * `remove` deletes one fact, one at a time, and `notice` tells how the last delete ended. A fact
 * leaves `state` only once the gear has said it is gone, and a list read that began before the
 * delete is dropped, so it cannot bring the fact back.
 */
export function useMemory(): Memory {
  const [state, setState] = useState<MemoryState>({ kind: 'reading' });
  const [notice, setNotice] = useState<DeleteNotice | null>(null);
  const [deleting, setDeleting] = useState(false);
  /** Counts the facts deleted; a read that began before a delete must not bring its row back. */
  const deleted = useRef(0);
  const busy = useRef(false);
  const remove = useCallback(async (factId: string, name: string): Promise<void> => {
    // The state of `deleting` shows after a render; this holds at once, for a second click.
    if (busy.current) {
      return;
    }
    busy.current = true;
    setDeleting(true);
    try {
      let reply;
      try {
        reply = await window.constructAgent.deleteMemoryFact(factId);
      } catch (error) {
        reply = { ok: false as const, error: message(error) };
      }
      if (!reply.ok) {
        setNotice({ kind: 'failed', fact: name, reason: reply.error });
        return;
      }
      deleted.current += 1;
      setNotice({ kind: 'deleted' });
      setState((current) =>
        current.kind === 'ready'
          ? { ...current, facts: current.facts.filter((fact) => fact.fact_id !== factId) }
          : current,
      );
    } finally {
      busy.current = false;
      setDeleting(false);
    }
  }, []);
  useEffect(() => {
    let cancelled = false;
    let version: number | null = null;
    let loaded = false;
    let busy = false;
    const fail = (reason: string): void => {
      loaded = false;
      if (!cancelled) {
        setState({ kind: 'failed', reason });
      }
    };
    const load = async (): Promise<void> => {
      const startedAt = deleted.current;
      const [facts, settings] = await Promise.all([
        window.constructAgent.getMemoryFacts(),
        window.constructAgent.getSettings(),
      ]);
      // A list read before a delete may still hold the fact: the next look reads it again.
      if (cancelled || startedAt !== deleted.current) {
        return;
      }
      if (!facts.ok) {
        fail(facts.error);
        return;
      }
      loaded = true;
      version = facts.version;
      setState({
        kind: 'ready',
        facts: facts.facts,
        collecting: settings.ok ? settings.collection.enabled : null,
      });
    };
    const look = async (): Promise<void> => {
      // A slow call is waited for, not stacked on.
      if (busy) {
        return;
      }
      busy = true;
      try {
        if (!loaded) {
          await load();
          return;
        }
        const reply = await window.constructAgent.getMemoryVersion();
        if (cancelled) {
          return;
        }
        if (!reply.ok) {
          fail(reply.error);
        } else if (reply.version !== version) {
          await load();
        }
      } catch (error) {
        fail(message(error));
      } finally {
        busy = false;
      }
    };
    void look();
    const timer = setInterval(() => {
      void look();
    }, versionPollMs);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);
  return { state, notice, deleting, remove };
}
