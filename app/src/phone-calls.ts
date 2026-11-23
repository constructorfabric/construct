import { callAgent, settingsLimits } from './agent-socket';
import type { Where } from './agent-socket';
import { parsePairing, parsePhoneAck, parsePhonePage, parsePhoneStatus } from './phone-protocol';
import type {
  PairingResult,
  PairingStatus,
  Phone,
  PhoneActionResult,
  PhonesResult,
} from './phone-protocol';

/** A roster screen groups up to 16 socket pages of eight phones each. */
const rosterLimits = { readMs: 30_000, maxPages: 16 };
const rosterUnavailable = { ok: false, error: 'Could not read the paired phones.' } as const;

/** Serializing open and close ensures a late open cannot outlive the screen. */
export function createPhoneHandlers(where: Where, roster = rosterLimits) {
  let pending: Promise<unknown> = Promise.resolve();
  let mayBeOpen = false;
  let queuedOpens = 0;
  const call = async <T>(
    line: string,
    parse: (line: string) => T,
    deadlineMs = settingsLimits.deadlineMs,
  ): Promise<T | { ok: false; error: string }> => {
    try {
      return parse(
        await callAgent(where.env, where.platform, line, { ...settingsLimits, deadlineMs }),
      );
    } catch {
      // Parsing errors can include the input; never expose a pairing payload in an error.
      return { ok: false, error: 'Could not contact the agent or read its reply.' };
    }
  };
  function serial<T>(operation: () => Promise<T>): Promise<T> {
    const result = pending.then(operation, operation);
    pending = result;
    return result;
  }
  return {
    needsClose: (): boolean => mayBeOpen || queuedOpens > 0,
    open: (): Promise<PairingResult> => {
      queuedOpens++;
      return serial(async () => {
        queuedOpens--;
        // The agent might open an offer even if its reply is lost.
        mayBeOpen = true;
        return call('phone-pairing-open', parsePairing);
      });
    },
    close: (): Promise<PhoneActionResult> =>
      serial(async () => {
        if (!mayBeOpen) return { ok: true };
        let result = await call('phone-pairing-close', parsePhoneAck);
        if (!result.ok) result = await call('phone-pairing-close', parsePhoneAck);
        if (result.ok) mayBeOpen = false;
        return result;
      }),
    status: (): Promise<PairingStatus> => call('phone-pairing-status', parsePhoneStatus),
    list: async (cursor: unknown = null): Promise<PhonesResult> => {
      if (cursor !== null && (typeof cursor !== 'string' || !cursor || cursor.length > 256)) {
        return rosterUnavailable;
      }
      const phones: Phone[] = [];
      let after = cursor;
      const deadline = performance.now() + roster.readMs;
      for (let pages = 0; pages < roster.maxPages; pages++) {
        const remaining = deadline - performance.now();
        if (remaining <= 0) return rosterUnavailable;
        const page = await call(
          `phone-list ${JSON.stringify({ after })}`,
          parsePhonePage,
          Math.min(settingsLimits.deadlineMs, remaining),
        );
        if (!page.ok) return page;
        phones.push(...page.phones);
        if (page.next !== null && after !== null && page.next <= after) return rosterUnavailable;
        after = page.next;
        if (after === null) break;
      }
      return { ok: true, phones: phones.sort((a, b) => a.pairedAt - b.pairedAt), next: after };
    },
    unpair: (id: unknown): Promise<PhoneActionResult> =>
      typeof id !== 'string' || id.length === 0 || id.length > 256
        ? Promise.resolve({ ok: false, error: 'Invalid phone ID.' })
        : call(`phone-unpair ${JSON.stringify({ id, ack_only: true })}`, parsePhoneAck),
  };
}
