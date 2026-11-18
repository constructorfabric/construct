import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import net from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { createDashboardHandlers } from '../src/dashboard-calls';

const day = { label: '2026-11-16', start: '2026-11-16T00:00:00Z', end: '2026-11-17T00:00:00Z' };

// A fake agent that records the lines it gets and answers each with `reply`.
async function withFakeAgent(
  reply: string,
  check: (
    handlers: ReturnType<typeof createDashboardHandlers>,
    lines: readonly string[],
  ) => Promise<void>,
): Promise<void> {
  const runtimeDir = mkdtempSync(path.join(tmpdir(), 'cb-unit-'));
  mkdirSync(path.join(runtimeDir, 'construct'));
  const lines: string[] = [];
  const server = net.createServer((client) => {
    client.setEncoding('utf8');
    client.once('data', (data: string) => {
      lines.push(data.trim());
      client.end(`${reply}\n`);
    });
  });
  await new Promise<void>((resolve) => {
    server.listen(path.join(runtimeDir, 'construct', 'agent.sock'), resolve);
  });
  try {
    await check(
      createDashboardHandlers({ env: { XDG_RUNTIME_DIR: runtimeDir }, platform: 'linux' }),
      lines,
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
    rmSync(runtimeDir, { recursive: true, force: true });
  }
}

describe('the dashboard handlers', () => {
  it('ask dashboard-numbers for the days and give the numbers', async () => {
    const reply =
      '{"total_minutes":30,"session_count":1,"hidden_project_count":0,"groups":[],"days":[{"label":"2026-11-16","minutes":30}]}';
    await withFakeAgent(reply, async (handlers, lines) => {
      const result = await handlers.numbers({ days: [day], sessions: 'unattended' });
      expect(result).toEqual({
        ok: true,
        totalMinutes: 30,
        sessionCount: 1,
        days: [{ label: '2026-11-16', minutes: 30 }],
      });
      expect(lines).toEqual([
        `dashboard-numbers ${JSON.stringify({ days: [day], sessions: 'unattended' })}`,
      ]);
    });
  });

  it('give the error of the agent as it is', async () => {
    await withFakeAgent('{"error":"the activity time could not be read"}', async (handlers) => {
      expect(await handlers.numbers({ days: [day], sessions: 'own' })).toEqual({
        ok: false,
        error: 'the activity time could not be read',
      });
    });
  });

  it('refuse a request the app does not accept, without calling the agent', async () => {
    await withFakeAgent('{}', async (handlers, lines) => {
      const result = await handlers.numbers({ days: [], sessions: 'own' });
      expect(result).toEqual({
        ok: false,
        error: 'the window sent a dashboard request in a form the app does not accept',
      });
      expect(lines).toEqual([]);
    });
  });

  it('say so when the agent is not there', async () => {
    const handlers = createDashboardHandlers({
      env: { XDG_RUNTIME_DIR: path.join(tmpdir(), 'cb-no-such-runtime-dir') },
      platform: 'linux',
    });
    const result = await handlers.numbers({ days: [day], sessions: 'own' });
    expect(result).toEqual({ ok: false, error: 'the agent is not running' });
  });
});
