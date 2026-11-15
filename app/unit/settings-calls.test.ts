import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import net from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { createSettingsHandlers } from '../src/settings-calls';

const secret = 'sk-secret-4f9a';

// A fake agent that records the lines it gets and answers each with `answer(line)`.
async function withFakeAgent(
  answer: (line: string) => string | null,
  check: (
    handlers: ReturnType<typeof createSettingsHandlers>,
    lines: readonly string[],
  ) => Promise<void>,
): Promise<void> {
  const runtimeDir = mkdtempSync(path.join(tmpdir(), 'cb-unit-'));
  mkdirSync(path.join(runtimeDir, 'construct'));
  const lines: string[] = [];
  const clients = new Set<net.Socket>();
  const server = net.createServer((client) => {
    clients.add(client);
    client.setEncoding('utf8');
    client.once('data', (data: string) => {
      const line = data.trim();
      lines.push(line);
      const reply = answer(line);
      if (reply !== null) {
        client.end(`${reply}\n`);
      }
    });
  });
  await new Promise<void>((resolve) => {
    server.listen(path.join(runtimeDir, 'construct', 'agent.sock'), resolve);
  });
  try {
    await check(
      createSettingsHandlers({ env: { XDG_RUNTIME_DIR: runtimeDir }, platform: 'linux' }),
      lines,
    );
  } finally {
    clients.forEach((client) => {
      client.destroy();
    });
    await new Promise((resolve) => server.close(resolve));
    rmSync(runtimeDir, { recursive: true, force: true });
  }
}

const saved = '{"model":{"base_url":"https://x/v1","model":"m","api_key_set":true}}';

describe('the settings handlers', () => {
  it('get asks settings-get and gives the settings, without any key', async () => {
    await withFakeAgent(
      () => saved,
      async (handlers, lines) => {
        expect(await handlers.get()).toEqual({
          ok: true,
          model: { baseUrl: 'https://x/v1', model: 'm', apiKeySet: true },
        });
        expect(lines).toEqual(['settings-get']);
      },
    );
  });

  it('save sends the typed key to the agent, and the result holds none', async () => {
    await withFakeAgent(
      () => saved,
      async (handlers, lines) => {
        const result = await handlers.save({ baseUrl: 'https://x/v1', model: 'm', apiKey: secret });
        expect(JSON.stringify(result)).not.toContain(secret);
        expect(lines[0]).toContain(secret);
      },
    );
  });

  it('refuses to send the saved key to another address, for save and for test', async () => {
    await withFakeAgent(
      () => saved,
      async (handlers, lines) => {
        const elsewhere = { baseUrl: 'https://attacker.example/v1', model: 'm' };
        expect(await handlers.save(elsewhere)).toEqual({
          ok: false,
          error: expect.stringContaining('type the key again') as string,
        });
        expect((await handlers.test(elsewhere)).result).toBe('failed');
        // Only the read of the saved settings went to the agent.
        expect(lines).toEqual(['settings-get', 'settings-get']);
      },
    );
  });

  it('lets another address through when a key is typed, and the saved address without one', async () => {
    await withFakeAgent(
      () => saved,
      async (handlers, lines) => {
        await handlers.save({ baseUrl: 'https://other/v1', model: 'm', apiKey: secret });
        await handlers.save({ baseUrl: 'https://x/v1', model: 'm' });
        expect(lines.map((line) => line.split(' ')[0])).toEqual([
          'settings-set',
          'settings-get',
          'settings-set',
        ]);
      },
    );
  });

  it('lets another address through when no key is saved', async () => {
    await withFakeAgent(
      (line) =>
        line === 'settings-get'
          ? '{"model":{"base_url":"https://x/v1","model":"m","api_key_set":false}}'
          : saved,
      async (handlers, lines) => {
        await handlers.save({ baseUrl: 'https://other/v1', model: 'm' });
        expect(lines.at(-1)).toContain('settings-set');
      },
    );
  });

  it('save with no key sends none', async () => {
    await withFakeAgent(
      () => saved,
      async (handlers, lines) => {
        await handlers.save({ baseUrl: 'https://x/v1', model: 'm' });
        expect(lines.at(-1)).toContain('settings-set');
        expect(lines.at(-1)).not.toContain('api_key');
      },
    );
  });

  it('save answers an agent refusal as a value', async () => {
    await withFakeAgent(
      () => '{"error":"base_url must be an https address"}',
      async (handlers) => {
        expect(
          await handlers.save({ baseUrl: 'http://example.com', model: 'm', apiKey: secret }),
        ).toEqual({
          ok: false,
          error: 'base_url must be an https address',
        });
      },
    );
  });

  it('refuses input of the wrong shape without asking the agent', async () => {
    await withFakeAgent(
      () => saved,
      async (handlers, lines) => {
        expect((await handlers.save({ baseUrl: 1 })).ok).toBe(false);
        expect((await handlers.test('x')).result).toBe('failed');
        expect(lines).toEqual([]);
      },
    );
  });

  it('removeKey sends the flag', async () => {
    await withFakeAgent(
      () => '{"model":{"base_url":"u","model":"m","api_key_set":false}}',
      async (handlers, lines) => {
        expect(await handlers.removeKey()).toEqual({
          ok: true,
          model: { baseUrl: 'u', model: 'm', apiKeySet: false },
        });
        expect(lines).toEqual(['settings-set {"model":{"remove_key":true}}']);
      },
    );
  });

  it('test gives the agent result and never repeats the key on a bad reply', async () => {
    await withFakeAgent(
      () => '{"result":"works","text":"pong"}',
      async (handlers) => {
        expect(await handlers.test({ baseUrl: 'u', model: 'm', apiKey: secret })).toEqual({
          result: 'works',
          text: 'pong',
        });
      },
    );
    await withFakeAgent(
      (line) => `not json ${line}`,
      async (handlers) => {
        const result = await handlers.test({ baseUrl: 'u', model: 'm', apiKey: secret });
        expect(result.result).toBe('failed');
        expect(JSON.stringify(result)).not.toContain(secret);
      },
    );
  });

  it('says the agent is not running when nobody listens', async () => {
    const runtimeDir = mkdtempSync(path.join(tmpdir(), 'cb-unit-'));
    try {
      const handlers = createSettingsHandlers({
        env: { XDG_RUNTIME_DIR: runtimeDir },
        platform: 'linux',
      });
      expect(await handlers.get()).toEqual({ ok: false, error: 'the agent is not running' });
    } finally {
      rmSync(runtimeDir, { recursive: true, force: true });
    }
  });
});
