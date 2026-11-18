import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import net from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { createDashboardSetupHandlers } from '../src/dashboard-setup-calls';

const idle = '{"state":"idle","sessions":0,"text_bytes":0,"sent":0,"received":0}';

// A fake agent that records the lines it gets and answers each with `reply`.
async function withFakeAgent(
  reply: string,
  check: (
    handlers: ReturnType<typeof createDashboardSetupHandlers>,
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
      createDashboardSetupHandlers({ env: { XDG_RUNTIME_DIR: runtimeDir }, platform: 'linux' }),
      lines,
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
    rmSync(runtimeDir, { recursive: true, force: true });
  }
}

describe('the dashboard setup handlers', () => {
  it('send one fixed command each', async () => {
    for (const [name, command] of [
      ['start', 'dashboard-setup-start {"days":30}'],
      ['status', 'dashboard-setup-status'],
      ['cancel', 'dashboard-setup-cancel'],
    ] as const) {
      await withFakeAgent(idle, async (handlers, lines) => {
        const result = await handlers[name]();
        expect(result.ok).toBe(true);
        expect(lines).toEqual([command]);
      });
    }
  });

  it('give the error of the agent as it is', async () => {
    await withFakeAgent('{"error":"no model is set"}', async (handlers) => {
      expect(await handlers.start()).toEqual({ ok: false, error: 'no model is set' });
    });
  });

  it('give a plain reason when nobody is there', async () => {
    const handlers = createDashboardSetupHandlers({
      env: { XDG_RUNTIME_DIR: path.join(tmpdir(), 'cb-nobody-here') },
      platform: 'linux',
    });
    const result = await handlers.status();
    expect(result.ok).toBe(false);
  });
});
