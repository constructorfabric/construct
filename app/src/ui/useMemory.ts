import { useEffect, useState } from 'react';
import type { MemoryFact } from '../memory-protocol';

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

const lostAgent = 'the window lost the agent';

function message(error: unknown): string {
  return error instanceof Error ? error.message : lostAgent;
}

/**
 * The facts of the person. While the page is open it asks for the profile's version now and then
 * (one fact per call) and reads the list again when the version is not the one it has, so a new
 * fact appears by itself. After a failure the next look reads the list again.
 */
export function useMemory(): MemoryState {
  const [state, setState] = useState<MemoryState>({ kind: 'reading' });
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
      const [facts, settings] = await Promise.all([
        window.constructAgent.getMemoryFacts(),
        window.constructAgent.getSettings(),
      ]);
      if (cancelled) {
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
  return state;
}
