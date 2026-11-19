import { describe, expect, it, vi } from 'vitest';
import { deleteAllChats } from '../src/ui/chat-delete';
import type { DeleteAllSteps } from '../src/ui/chat-delete';
import type { DeleteResult } from '../src/chat-protocol';

const quick = { endWaitMs: 10, busyRetryMs: 1, busyRetries: 3 };
const ok: DeleteResult = { ok: true };
const busy: DeleteResult = { ok: false, error: 'busy' };

function steps(running: boolean, answers: DeleteResult[]): DeleteAllSteps & { calls: string[] } {
  const calls: string[] = [];
  return {
    calls,
    running: () => running,
    stopAndWait: vi.fn(async () => {
      calls.push('stop');
    }),
    remove: vi.fn(async () => {
      calls.push('remove');
      return answers.shift() ?? ok;
    }),
  };
}

describe('deleteAllChats', () => {
  it('only deletes when no answer runs', async () => {
    const run = steps(false, [ok]);
    expect(await deleteAllChats(run, quick)).toEqual(ok);
    expect(run.calls).toEqual(['remove']);
  });

  it('stops a running answer and waits for it before it deletes', async () => {
    const run = steps(true, [ok]);
    await deleteAllChats(run, quick);
    expect(run.calls).toEqual(['stop', 'remove']);
  });

  it('asks again when the agent says busy just after the end', async () => {
    const run = steps(true, [busy, busy, ok]);
    expect(await deleteAllChats(run, quick)).toEqual(ok);
    expect(run.calls).toEqual(['stop', 'remove', 'remove', 'remove']);
  });

  it('gives the agent reason when it is still busy after the retries', async () => {
    const run = steps(true, [busy, busy, busy, busy, busy]);
    expect(await deleteAllChats(run, quick)).toEqual(busy);
    expect(run.calls.filter((call) => call === 'remove')).toHaveLength(4);
  });

  it('does not ask again for a reason other than busy', async () => {
    const run = steps(false, [{ ok: false, error: 'cannot delete' }]);
    expect(await deleteAllChats(run, quick)).toEqual({ ok: false, error: 'cannot delete' });
    expect(run.calls).toEqual(['remove']);
  });
});
