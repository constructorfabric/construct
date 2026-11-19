import { useCallback, useEffect, useRef, useState } from 'react';
import { t } from '../translation';

/** How long the page waits between two looks at the gear while it erases. */
const erasureCheckMs = 1000;
/** The longest wait for the end of an erasure, counted from the agent's answer. */
const erasureWaitMs = 180_000;

/** What "Delete everything" is doing: nothing yet, asking the agent, waiting for the gear, or at its end. */
export type Erasure =
  | { readonly kind: 'idle' }
  | { readonly kind: 'asking' }
  /** `refused` is what the agent said when it answered with an error but the gear is erasing. */
  | { readonly kind: 'waiting'; readonly refused: string | null }
  | { readonly kind: 'done' }
  /** `refused` is the agent's own reason, which a later look at the gear must not turn into a success. */
  | { readonly kind: 'failed'; readonly reason: string; readonly refused: string | null };

export interface Erasing {
  readonly state: Erasure;
  /** Counts the answers of the agent to `memory-erase`, so the page can read its settings again. */
  readonly answers: number;
  readonly start: () => Promise<void>;
  /** Looks at the gear: an erasure that runs, or that ran on after a failure, is waited for again. */
  readonly look: () => void;
}

/**
 * The erasure of everything about the person, kept above the pages so that leaving Settings
 * neither loses the wait nor its end. It asks the agent, then looks at the gear once a second
 * while it says `erasure_in_progress`. Only an erasure the agent started cleanly ends in `done`.
 */
export function useErasing(reload: () => void): Erasing {
  const [state, setState] = useState<Erasure>({ kind: 'idle' });
  const [answers, setAnswers] = useState(0);
  // What `start` reads when it runs, so that a press that gets past a closed button is refused.
  const latest = useRef(state);
  latest.current = state;

  // An erasure that runs is waited for, whenever the app looks: at launch, and at each visit to
  // Settings, since the host may not have been announced before.
  const look = useCallback(() => {
    void window.constructAgent.getErasing().then((result) => {
      if (!result.ok || !result.erasing) {
        return;
      }
      setState((current) => {
        if (current.kind === 'idle') {
          return { kind: 'waiting', refused: null };
        }
        return current.kind === 'failed' ? { kind: 'waiting', refused: current.refused } : current;
      });
    });
  }, []);
  useEffect(look, [look]);

  // While the gear says it erases, the page looks again, one look after the other.
  useEffect(() => {
    if (state.kind !== 'waiting') {
      return undefined;
    }
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const giveUp = Date.now() + erasureWaitMs;
    const { refused } = state;
    const poll = (): void => {
      void window.constructAgent.getErasing().then((result) => {
        if (cancelled) {
          return;
        }
        if (!result.ok) {
          setState({ kind: 'failed', reason: result.error, refused });
        } else if (result.erasing && Date.now() >= giveUp) {
          setState({ kind: 'failed', reason: t('YourData.TooLong'), refused });
        } else if (result.erasing) {
          timer = setTimeout(poll, erasureCheckMs);
        } else {
          setState(
            refused === null ? { kind: 'done' } : { kind: 'failed', reason: refused, refused },
          );
        }
      });
    };
    poll();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [state]);

  const start = useCallback(async () => {
    if (latest.current.kind === 'asking' || latest.current.kind === 'waiting') {
      return;
    }
    // A press after a failure that left the gear erasing: the agent then says `running`, and
    // that is the same erasure, so its own reason is still the end of it.
    const carried = latest.current.kind === 'failed' ? latest.current.refused : null;
    setState({ kind: 'asking' });
    const result = await window.constructAgent.eraseEverything();
    // The agent may have wiped the chats and switched collection off before it answered, an error
    // included, so the list and the switches are read again whatever the answer is.
    reload();
    setAnswers((count) => count + 1);
    if (result.ok) {
      setState({ kind: 'waiting', refused: result.running ? carried : null });
      return;
    }
    setState({ kind: 'failed', reason: result.error, refused: result.error });
    // An answer that came late or failed may leave the gear erasing: the wait goes on, and the agent's reason stays the end.
    const erasing = await window.constructAgent.getErasing();
    if (erasing.ok && erasing.erasing) {
      setState((current) =>
        current.kind === 'failed' ? { kind: 'waiting', refused: result.error } : current,
      );
    }
  }, [reload]);

  return { state, answers, start, look };
}
