import type { DeleteResult } from '../chat-protocol';

/** What Delete all does with the agent; each step is the app's own call, so a test can stand in. */
export interface DeleteAllSteps {
  /** Whether an answer is running now. */
  readonly running: () => boolean;
  /** Stops the answer and gives when it has ended, or when `ms` have passed. */
  readonly stopAndWait: (ms: number) => Promise<void>;
  readonly remove: () => Promise<DeleteResult>;
}

export interface DeleteAllTiming {
  /** How long a stopped answer is waited for. */
  readonly endWaitMs: number;
  /** How long to wait before asking the agent again when it says `busy`, and how many times. */
  readonly busyRetryMs: number;
  readonly busyRetries: number;
}

export const deleteAllTiming: DeleteAllTiming = {
  endWaitMs: 3000,
  busyRetryMs: 100,
  busyRetries: 20,
};

/**
 * Deletes every conversation. The agent does not delete under a running answer, so a running one
 * is stopped and waited for first. The agent frees its place for the next answer just after it
 * says the answer ended, so a `busy` right after is asked again for a short while.
 */
export async function deleteAllChats(
  steps: DeleteAllSteps,
  timing: DeleteAllTiming = deleteAllTiming,
): Promise<DeleteResult> {
  if (steps.running()) {
    await steps.stopAndWait(timing.endWaitMs);
  }
  let result = await steps.remove();
  for (
    let retry = 0;
    !result.ok && result.error === 'busy' && retry < timing.busyRetries;
    retry += 1
  ) {
    await new Promise((resolve) => setTimeout(resolve, timing.busyRetryMs));
    result = await steps.remove();
  }
  return result;
}
