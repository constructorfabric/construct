/**
 * A quarter of a second is about what a person takes to notice that a page is waiting; what shows
 * then stays for half a second.
 */
export const loaderTiming = { delayMs: 250, minShownMs: 500 } as const;

export interface DelayedFlag {
  /** Tells the flag whether the page waits now. */
  readonly setWaiting: (waiting: boolean) => void;
  /** Stops the timers; nothing is reported after it. */
  readonly dispose: () => void;
}

/**
 * A flag that turns on only after the wait has lasted `delayMs`, and, once on, stays on for at
 * least `minShownMs`, so that a quick answer shows nothing and a slow one does not blink.
 */
export function createDelayedFlag(onChange: (shown: boolean) => void): DelayedFlag {
  let waiting = false;
  let shown = false;
  let shownAt = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const stopTimer = (): void => {
    clearTimeout(timer);
    timer = undefined;
  };
  const turn = (on: boolean): void => {
    shown = on;
    shownAt = Date.now();
    onChange(on);
  };

  return {
    setWaiting: (next) => {
      if (next === waiting) {
        return;
      }
      waiting = next;
      stopTimer();
      if (waiting) {
        // A wait that starts again while the flag is still on keeps it on.
        if (!shown) {
          timer = setTimeout(() => {
            turn(true);
          }, loaderTiming.delayMs);
        }
        return;
      }
      if (!shown) {
        return;
      }
      const left = loaderTiming.minShownMs - (Date.now() - shownAt);
      if (left <= 0) {
        turn(false);
        return;
      }
      timer = setTimeout(() => {
        turn(false);
      }, left);
    },
    dispose: stopTimer,
  };
}
