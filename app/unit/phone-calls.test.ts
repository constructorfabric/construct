import { describe, expect, it, vi } from 'vitest';
import { createPhoneHandlers } from '../src/phone-calls';
import { startFakeAgent } from './fake-agent';

const offer = {
  payload: 'test-payload',
  code: '12345678',
  address: 'host:123',
  expires_in: 300,
};

describe('phone calls', () => {
  it('opens, lists, unpairs and closes using the local protocol', async () => {
    const agent = await startFakeAgent((line) => {
      if (line === 'phone-pairing-open') return JSON.stringify(offer);
      if (line.startsWith('phone-list '))
        return JSON.stringify({
          phones: [{ id: 'one', name: 'Phone', paired_at: 42 }],
          next: null,
        });
      if (line === 'phone-pairing-status') return '{"active":true}';
      return JSON.stringify({ ok: true });
    });
    try {
      const calls = createPhoneHandlers({
        env: { XDG_RUNTIME_DIR: agent.runtimeDir },
        platform: 'linux',
      });
      expect(await calls.open()).toEqual({
        ok: true,
        payload: offer.payload,
        code: '12345678',
        address: 'host:123',
        expiresIn: 300,
      });
      expect(await calls.list()).toEqual({
        ok: true,
        phones: [{ id: 'one', name: 'Phone', pairedAt: 42 }],
        next: null,
      });
      expect(await calls.status()).toEqual({ ok: true, active: true });
      expect(await calls.unpair('one')).toEqual({ ok: true });
      expect(await calls.close()).toEqual({ ok: true });
      expect(agent.lines).toEqual([
        'phone-pairing-open',
        'phone-list {"after":null}',
        'phone-pairing-status',
        'phone-unpair {"id":"one","ack_only":true}',
        'phone-pairing-close',
      ]);
    } finally {
      await agent.close();
    }
  });

  it('waits for an open to finish before closing', async () => {
    let release!: (value: string) => void;
    const answer = new Promise<string>((resolve) => {
      release = resolve;
    });
    let sawOpen!: () => void;
    const openSeen = new Promise<void>((resolve) => {
      sawOpen = resolve;
    });
    const agent = await startFakeAgent((line) => {
      if (line === 'phone-pairing-open') {
        sawOpen();
        return answer;
      }
      if (line.startsWith('phone-list ')) return '{"phones":[],"next":null}';
      return '{"ok":true}';
    });
    try {
      const calls = createPhoneHandlers({
        env: { XDG_RUNTIME_DIR: agent.runtimeDir },
        platform: 'linux',
      });
      expect(calls.needsClose()).toBe(false);
      expect(await calls.close()).toEqual({ ok: true });
      expect(agent.lines).toEqual([]);
      const opening = calls.open();
      await openSeen;
      expect(calls.needsClose()).toBe(true);
      const closing = calls.close();
      // A roster round trip can complete while the open reply is still held.
      expect(await calls.list()).toEqual({ ok: true, phones: [], next: null });
      expect(agent.lines).toEqual(['phone-pairing-open', 'phone-list {"after":null}']);
      release(JSON.stringify(offer));
      await opening;
      expect(await closing).toEqual({ ok: true });
      expect(agent.lines).toEqual([
        'phone-pairing-open',
        'phone-list {"after":null}',
        'phone-pairing-close',
      ]);
      expect(calls.needsClose()).toBe(false);
    } finally {
      await agent.close();
    }
  });

  it('rejects malformed replies without exposing their contents and refuses invalid IDs', async () => {
    const agent = await startFakeAgent(() => '{"payload":"secret"');
    try {
      const calls = createPhoneHandlers({
        env: { XDG_RUNTIME_DIR: agent.runtimeDir },
        platform: 'linux',
      });
      expect(JSON.stringify(await calls.open())).not.toContain('secret');
      expect((await calls.list()).ok).toBe(false);
      expect((await calls.list({ after: 'secret' })).ok).toBe(false);
      expect((await calls.unpair('')).ok).toBe(false);
      expect(agent.lines).toEqual(['phone-pairing-open', 'phone-list {"after":null}']);
    } finally {
      await agent.close();
    }
  });

  it('retries a failed close and keeps cleanup pending until it is acknowledged', async () => {
    let closeAttempts = 0;
    const agent = await startFakeAgent((line) => {
      if (line === 'phone-pairing-open') return JSON.stringify(offer);
      closeAttempts++;
      return closeAttempts <= 2 ? '{"error":"try again"}' : '{"ok":true}';
    });
    try {
      const calls = createPhoneHandlers({
        env: { XDG_RUNTIME_DIR: agent.runtimeDir },
        platform: 'linux',
      });
      await calls.open();
      expect(await calls.close()).toEqual({ ok: false, error: 'try again' });
      expect(closeAttempts).toBe(2);
      expect(calls.needsClose()).toBe(true);
      expect(await calls.close()).toEqual({ ok: true });
      expect(calls.needsClose()).toBe(false);
    } finally {
      await agent.close();
    }
  });

  it('collects multiple bounded pages and unpairs without a roster reply', async () => {
    const rows = Array.from({ length: 32 }, (_, index) => ({
      id: String(index).padStart(2, '0'),
      name: '📱'.repeat(64),
      paired_at: 32 - index,
    }));
    const agent = await startFakeAgent((line) => {
      if (line.startsWith('phone-unpair ')) return '{"ok":true}';
      const { after } = JSON.parse(line.slice('phone-list '.length)) as { after: string | null };
      const start = after === null ? 0 : Number(after) + 1;
      const phones = rows.slice(start, start + 8);
      return JSON.stringify({ phones, next: start + 8 < rows.length ? phones.at(-1)?.id : null });
    });
    try {
      const calls = createPhoneHandlers({
        env: { XDG_RUNTIME_DIR: agent.runtimeDir },
        platform: 'linux',
      });
      expect(JSON.stringify({ phones: rows }).length).toBeGreaterThan(4096);
      const result = await calls.list();
      expect(result.ok && result.phones.map((phone) => phone.id)).toEqual(
        rows.map((phone) => phone.id).reverse(),
      );
      expect(await calls.unpair('10')).toEqual({ ok: true });
      expect(agent.lines).toEqual([
        'phone-list {"after":null}',
        'phone-list {"after":"07"}',
        'phone-list {"after":"15"}',
        'phone-list {"after":"23"}',
        'phone-unpair {"id":"10","ack_only":true}',
      ]);
    } finally {
      await agent.close();
    }
  });

  it('returns the cursor at the page limit so the next call can continue', async () => {
    let id = 0;
    const agent = await startFakeAgent(() => {
      const cursor = String(++id);
      return JSON.stringify({
        phones: [{ id: cursor, name: 'Phone', paired_at: id }],
        next: id < 3 ? cursor : null,
      });
    });
    try {
      const calls = createPhoneHandlers(
        { env: { XDG_RUNTIME_DIR: agent.runtimeDir }, platform: 'linux' },
        { readMs: 30_000, maxPages: 2 },
      );
      const first = await calls.list();
      expect(first.ok && first.phones.map((phone) => phone.id)).toEqual(['1', '2']);
      expect(first.ok && first.next).toBe('2');
      const last = await calls.list(first.ok ? first.next : null);
      expect(last.ok && last.phones.map((phone) => phone.id)).toEqual(['3']);
      expect(last.ok && last.next).toBeNull();
      expect(agent.lines).toEqual([
        'phone-list {"after":null}',
        'phone-list {"after":"1"}',
        'phone-list {"after":"2"}',
      ]);
    } finally {
      await agent.close();
    }
  });

  it('does not ask for another page after the shared deadline', async () => {
    let now = 0;
    const agent = await startFakeAgent(() => {
      now = 100;
      return '{"phones":[],"next":"1"}';
    });
    const clock = vi.spyOn(performance, 'now').mockImplementation(() => now);
    try {
      const calls = createPhoneHandlers(
        { env: { XDG_RUNTIME_DIR: agent.runtimeDir }, platform: 'linux' },
        { readMs: 100, maxPages: 5 },
      );
      expect(await calls.list()).toEqual({ ok: false, error: 'Could not read the paired phones.' });
      expect(agent.lines).toEqual(['phone-list {"after":null}']);
    } finally {
      clock.mockRestore();
      await agent.close();
    }
  });

  it('closes a pending page request when the remaining roster budget expires', async () => {
    let now = 0;
    let sawPage!: () => void;
    let sawClose!: () => void;
    const pageSeen = new Promise<void>((resolve) => { sawPage = resolve; });
    const pageClosed = new Promise<void>((resolve) => { sawClose = resolve; });
    const agent = await startFakeAgent((line, client) => {
      if (line === 'phone-list {"after":null}') {
        now = 80;
        return '{"phones":[],"next":"1"}';
      }
      client.once('close', sawClose);
      sawPage();
      return null;
    });
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    const clock = vi.spyOn(performance, 'now').mockImplementation(() => now);
    try {
      const calls = createPhoneHandlers(
        { env: { XDG_RUNTIME_DIR: agent.runtimeDir }, platform: 'linux' },
        { readMs: 100, maxPages: 5 },
      );
      let settled = false;
      const listing = calls.list().then((result) => {
        settled = true;
        return result;
      });
      await pageSeen;
      await vi.advanceTimersByTimeAsync(19);
      expect(settled).toBe(false);
      await vi.advanceTimersByTimeAsync(1);
      expect(settled).toBe(true);
      expect(await listing).toEqual({
        ok: false,
        error: 'Could not contact the agent or read its reply.',
      });
      await pageClosed;
      expect(agent.lines).toEqual(['phone-list {"after":null}', 'phone-list {"after":"1"}']);
    } finally {
      clock.mockRestore();
      vi.useRealTimers();
      await agent.close();
    }
  });
});
