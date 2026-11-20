import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createDelayedFlag, loaderTiming } from '../src/ui/components/delayed-flag';
import type { DelayedFlag } from '../src/ui/components/delayed-flag';

describe('createDelayedFlag', () => {
  const changes: boolean[] = [];
  let flag: DelayedFlag;

  beforeEach(() => {
    vi.useFakeTimers();
    changes.length = 0;
    flag = createDelayedFlag((shown) => changes.push(shown));
  });

  afterEach(() => {
    flag.dispose();
    vi.useRealTimers();
  });

  it('is a quarter of a second to show and half a second to stay', () => {
    expect(loaderTiming).toEqual({ delayMs: 250, minShownMs: 500 });
  });

  it('shows nothing for an answer that comes before the delay', () => {
    flag.setWaiting(true);
    vi.advanceTimersByTime(loaderTiming.delayMs - 1);
    flag.setWaiting(false);
    vi.advanceTimersByTime(10_000);
    expect(changes).toEqual([]);
  });

  it('shows once the wait has lasted the delay', () => {
    flag.setWaiting(true);
    vi.advanceTimersByTime(loaderTiming.delayMs - 1);
    expect(changes).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(changes).toEqual([true]);
  });

  it('stays on for as long as one wait lasts', () => {
    flag.setWaiting(true);
    vi.advanceTimersByTime(10_000);
    expect(changes).toEqual([true]);
    flag.setWaiting(false);
    expect(changes).toEqual([true, false]);
  });

  it('stays for the minimum time when the answer comes right after it shows', () => {
    flag.setWaiting(true);
    vi.advanceTimersByTime(loaderTiming.delayMs);
    flag.setWaiting(false);
    vi.advanceTimersByTime(loaderTiming.minShownMs - 1);
    expect(changes).toEqual([true]);
    vi.advanceTimersByTime(1);
    expect(changes).toEqual([true, false]);
  });

  it('goes off at once when it has been on for the minimum time already', () => {
    flag.setWaiting(true);
    vi.advanceTimersByTime(loaderTiming.delayMs + loaderTiming.minShownMs);
    flag.setWaiting(false);
    expect(changes).toEqual([true, false]);
  });

  it('stays on when the wait starts again while it is held', () => {
    flag.setWaiting(true);
    vi.advanceTimersByTime(loaderTiming.delayMs);
    flag.setWaiting(false);
    vi.advanceTimersByTime(100);
    flag.setWaiting(true);
    vi.advanceTimersByTime(10_000);
    expect(changes).toEqual([true]);
    flag.setWaiting(false);
    expect(changes).toEqual([true, false]);
  });

  it('counts the delay again for a second wait', () => {
    flag.setWaiting(true);
    vi.advanceTimersByTime(loaderTiming.delayMs + loaderTiming.minShownMs);
    flag.setWaiting(false);
    flag.setWaiting(true);
    vi.advanceTimersByTime(loaderTiming.delayMs - 1);
    expect(changes).toEqual([true, false]);
    vi.advanceTimersByTime(1);
    expect(changes).toEqual([true, false, true]);
  });

  it('reports nothing after it is disposed', () => {
    flag.setWaiting(true);
    flag.dispose();
    vi.advanceTimersByTime(10_000);
    expect(changes).toEqual([]);
  });
});
