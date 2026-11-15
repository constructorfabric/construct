import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentStatus } from '../src/agent-status';
import { startStatusPoller } from '../src/status-poller';

const running: AgentStatus = {
  state: 'running',
  version: '0.1.0',
  pid: 7,
  startAtLogin: 'unknown',
};

describe('startStatusPoller', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('reports at once and then at every interval', async () => {
    const seen: AgentStatus[] = [];
    const poller = startStatusPoller(
      () => Promise.resolve(running),
      (s) => seen.push(s),
      1000,
    );

    await vi.advanceTimersByTimeAsync(0);
    expect(seen).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(2000);
    expect(seen).toHaveLength(3);
    poller.stop();
  });

  it('asks again at once on refresh, but not while an ask is out', async () => {
    const seen: AgentStatus[] = [];
    const poller = startStatusPoller(
      () => Promise.resolve(running),
      (s) => seen.push(s),
      60_000,
    );
    await vi.advanceTimersByTimeAsync(0);

    poller.refresh();
    poller.refresh();
    await vi.advanceTimersByTimeAsync(0);
    expect(seen).toHaveLength(2);
    poller.stop();
  });

  it('turns a failed poll into an unknown status with the reason', async () => {
    const seen: AgentStatus[] = [];
    const poller = startStatusPoller(
      () => Promise.reject(new Error('bridge gone')),
      (s) => seen.push(s),
      1000,
    );

    await vi.advanceTimersByTimeAsync(0);
    expect(seen).toEqual([{ state: 'unknown', reason: 'bridge gone', startAtLogin: 'unknown' }]);
    poller.stop();
  });

  it('starts no poll while the previous one is still running', async () => {
    const getStatus = vi.fn(() => new Promise<AgentStatus>(() => undefined));
    const poller = startStatusPoller(getStatus, () => undefined, 1000);

    await vi.advanceTimersByTimeAsync(5000);
    expect(getStatus).toHaveBeenCalledTimes(1);
    poller.stop();
  });

  it('reports nothing after it is stopped', async () => {
    const seen: AgentStatus[] = [];
    const getStatus = vi.fn(() => Promise.resolve(running));
    const poller = startStatusPoller(getStatus, (s) => seen.push(s), 1000);
    await vi.advanceTimersByTimeAsync(0);

    poller.stop();
    await vi.advanceTimersByTimeAsync(5000);

    expect(seen).toHaveLength(1);
    expect(getStatus).toHaveBeenCalledTimes(1);
  });
});
