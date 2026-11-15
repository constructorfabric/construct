import type { AgentStatus } from './agent-status';

export const refreshMs = 3000;

export interface StatusPoller {
  /** Asks now instead of at the next tick; nothing happens while an ask is out. */
  readonly refresh: () => void;
  readonly stop: () => void;
}

// A poll starts only when the previous one has finished; the main process bounds each one.
export function startStatusPoller(
  getStatus: () => Promise<AgentStatus>,
  onStatus: (status: AgentStatus) => void,
  intervalMs: number = refreshMs,
): StatusPoller {
  let inFlight = false;
  let stopped = false;

  const refresh = async (): Promise<void> => {
    if (inFlight) {
      return;
    }
    inFlight = true;
    try {
      const status = await getStatus();
      if (!stopped) {
        onStatus(status);
      }
    } catch (error) {
      if (!stopped) {
        onStatus({
          state: 'unknown',
          reason: error instanceof Error ? error.message : String(error),
          startAtLogin: 'unknown',
        });
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
