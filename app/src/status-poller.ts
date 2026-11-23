import type { AgentStatus } from './agent-status';

export const refreshMs = 3000;

export interface StatusPoller {
  /** Asks now instead of at the next tick; nothing happens while an ask is out. */
  readonly refresh: () => void;
  readonly stop: () => void;
}

/** Poll one request at a time and resume after a rejected request. */
export function startPoller<T>(
  read: () => Promise<T>,
  onResult: (result: T) => void,
  intervalMs: number,
  onError?: (error: unknown) => void,
): StatusPoller {
  let inFlight = false;
  let stopped = false;

  const refresh = async (): Promise<void> => {
    if (inFlight || stopped) {
      return;
    }
    inFlight = true;
    try {
      const result = await read();
      if (!stopped) {
        onResult(result);
      }
    } catch (error) {
      if (!stopped) {
        onError?.(error);
      }
    } finally {
      inFlight = false;
    }
  };

  void refresh();
  const timer = setInterval(() => {
    void refresh();
  }, intervalMs);

  return {
    refresh: () => {
      void refresh();
    },
    stop: () => {
      stopped = true;
      clearInterval(timer);
    },
  };
}

export function startStatusPoller(
  getStatus: () => Promise<AgentStatus>,
  onStatus: (status: AgentStatus) => void,
  intervalMs: number = refreshMs,
): StatusPoller {
  return startPoller(getStatus, onStatus, intervalMs, (error) => {
    onStatus({
      state: 'unknown',
      reason: error instanceof Error ? error.message : String(error),
      startAtLogin: 'unknown',
    });
  });
}
