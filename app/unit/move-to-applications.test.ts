import { describe, expect, it, vi } from 'vitest';
import { offerMoveToApplications } from '../src/move-to-applications';
import type { MoveOffer } from '../src/move-to-applications';

function offer(over: Partial<MoveOffer> = {}): MoveOffer & {
  readonly marked: () => number;
  readonly handlers: (() => boolean)[];
} {
  let marks = 0;
  const handlers: (() => boolean)[] = [];
  return {
    isEligible: () => true,
    isDeclined: () => false,
    markDeclined: () => {
      marks += 1;
    },
    ask: () => Promise.resolve(true),
    move: (handler) => {
      handlers.push(handler);
      return true;
    },
    ...over,
    marked: () => marks,
    handlers,
  };
}

describe('offerMoveToApplications', () => {
  it('asks nothing when the user declined before', async () => {
    const ask = vi.fn(() => Promise.resolve(true));
    const move = vi.fn(() => true);
    expect(await offerMoveToApplications(offer({ isDeclined: () => true, ask, move }))).toBe(false);
    expect(ask).not.toHaveBeenCalled();
    expect(move).not.toHaveBeenCalled();
  });

  it('remembers "Not now" and does not move', async () => {
    const o = offer({ ask: () => Promise.resolve(false) });
    expect(await offerMoveToApplications(o)).toBe(false);
    expect(o.marked()).toBe(1);
    expect(o.handlers).toHaveLength(0);
  });

  it('moves with a conflict handler that keeps an installed copy', async () => {
    const o = offer();
    expect(await offerMoveToApplications(o)).toBe(true);
    expect(o.handlers.map((handler) => handler())).toEqual([false]);
    expect(o.marked()).toBe(0);
  });

  it('remembers a move that returned false', async () => {
    const o = offer({ move: () => false });
    expect(await offerMoveToApplications(o)).toBe(false);
    expect(o.marked()).toBe(1);
  });

  it('remembers a move that threw', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const o = offer({
      move: () => {
        throw new Error('no');
      },
    });
    expect(await offerMoveToApplications(o)).toBe(false);
    expect(o.marked()).toBe(1);
    error.mockRestore();
  });

  it('goes on when the marker cannot be written', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const o = offer({
      ask: () => Promise.resolve(false),
      markDeclined: () => {
        throw new Error('read-only');
      },
    });
    expect(await offerMoveToApplications(o)).toBe(false);
    expect(error).toHaveBeenCalledTimes(1);
    expect(o.marked()).toBe(0);
    error.mockRestore();
  });

  it('goes on when the question fails', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const o = offer({ ask: () => Promise.reject(new Error('no window')) });
    expect(await offerMoveToApplications(o)).toBe(false);
    // A failed dialog is not a refusal: the question comes back at the next launch.
    expect(o.marked()).toBe(0);
    expect(error).toHaveBeenCalledTimes(1);
    error.mockRestore();
  });

  it('asks nothing when the app is not eligible; a failing eligibility check does not reject', async () => {
    const ask = vi.fn(() => Promise.resolve(true));
    expect(await offerMoveToApplications(offer({ isEligible: () => false, ask }))).toBe(false);
    expect(ask).not.toHaveBeenCalled();
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const throwing = offer({
      isEligible: () => {
        throw new Error('electron');
      },
    });
    expect(await offerMoveToApplications(throwing)).toBe(false);
    error.mockRestore();
  });
});
