import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import net from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { describe, expect, it, vi } from 'vitest';
import type { TestProgress } from '../src/agent-settings';
import { createSettingsHandlers } from '../src/settings-calls';

const secret = 'sk-secret-4f9a';

// A fake agent that records the lines it gets and answers each with `answer(line)`, once the
// answer is there: a promise holds the reply back.
async function withFakeAgent(
  answer: (line: string) => string | null | Promise<string | null>,
  check: (
    handlers: ReturnType<typeof createSettingsHandlers>,
    lines: readonly string[],
    heard: readonly TestProgress[],
  ) => Promise<void>,
): Promise<void> {
  const runtimeDir = mkdtempSync(path.join(tmpdir(), 'cb-unit-'));
  mkdirSync(path.join(runtimeDir, 'construct'));
  const lines: string[] = [];
  const heard: TestProgress[] = [];
  const clients = new Set<net.Socket>();
  const server = net.createServer((client) => {
    clients.add(client);
    client.setEncoding('utf8');
    client.once('data', (data: string) => {
      const line = data.trim();
      lines.push(line);
      void Promise.resolve(answer(line)).then((reply) => {
        if (reply !== null) {
          client.end(`${reply}\n`);
        }
      });
    });
  });
  await new Promise<void>((resolve) => {
    server.listen(path.join(runtimeDir, 'construct', 'agent.sock'), resolve);
  });
  try {
    await check(
      createSettingsHandlers(
        { env: { XDG_RUNTIME_DIR: runtimeDir }, platform: 'linux' },
        (event) => {
          heard.push(event);
        },
      ),
      lines,
      heard,
    );
  } finally {
    clients.forEach((client) => {
      client.destroy();
    });
    await new Promise((resolve) => server.close(resolve));
    rmSync(runtimeDir, { recursive: true, force: true });
  }
}

const collection = '"collection":{"enabled":true,"claude_code":true,"codex":false},"notice":null';
const saved = `{"model":{"base_url":"https://x/v1","model":"m","api_key_set":true},${collection}}`;
const collectionOut = {
  collection: { enabled: true, claudeCode: true, codex: false },
  notice: null,
};

describe('the settings handlers', () => {
  it('get asks settings-get and gives the settings, without any key', async () => {
    await withFakeAgent(
      () => saved,
      async (handlers, lines) => {
        expect(await handlers.get()).toEqual({
          ok: true,
          model: { baseUrl: 'https://x/v1', model: 'm', apiKeySet: true },
          ...collectionOut,
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
          ? `{"model":{"base_url":"https://x/v1","model":"m","api_key_set":false},${collection}}`
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
      () => `{"model":{"base_url":"u","model":"m","api_key_set":false},${collection}}`,
      async (handlers, lines) => {
        expect(await handlers.removeKey()).toEqual({
          ok: true,
          model: { baseUrl: 'u', model: 'm', apiKeySet: false },
          ...collectionOut,
        });
        expect(lines).toEqual(['settings-set {"model":{"remove_key":true}}']);
      },
    );
  });

  it('saveCollection sends the switches under the agent names and gives the answer', async () => {
    await withFakeAgent(
      () => saved,
      async (handlers, lines) => {
        expect(await handlers.saveCollection({ claudeCode: false, enabled: true })).toEqual({
          ok: true,
          model: { baseUrl: 'https://x/v1', model: 'm', apiKeySet: true },
          ...collectionOut,
        });
        expect(lines).toEqual(['settings-set {"collection":{"enabled":true,"claude_code":false}}']);
      },
    );
  });

  it.each([
    ['null', null],
    ['text', 'x'],
    ['no switch', {}],
    ['a switch that is not a boolean', { codex: 'yes' }],
    ['a valid switch beside an invalid one', { enabled: true, codex: 'yes' }],
    ['only a name the app does not know', { claude_code: false }],
  ])('saveCollection refuses %s without asking the agent', async (_name, value) => {
    await withFakeAgent(
      () => saved,
      async (handlers, lines) => {
        expect((await handlers.saveCollection(value)).ok).toBe(false);
        expect(lines).toEqual([]);
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
          checks: [],
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

  it('test asks for the check lines and tells each one as it comes, then gives the last', async () => {
    const answer = [
      '{"checks":["answers","uses tools"]}',
      '{"check":{"name":"answers","result":"works"}}',
      '{"check":{"name":"uses tools","result":"no tool call"}}',
      '{"result":"no tool call","checks":[{"name":"answers","result":"works"},' +
        '{"name":"uses tools","result":"no tool call"}]}',
    ].join('\n');
    await withFakeAgent(
      () => answer,
      async (handlers, lines, heard) => {
        const result = await handlers.test({ baseUrl: 'u', model: 'm', apiKey: 'k' });
        expect(lines).toEqual([
          'settings-test {"base_url":"u","model":"m","api_key":"k","progress":true}',
        ]);
        expect(heard).toEqual([
          { event: 'checks', names: ['answers', 'uses tools'] },
          { event: 'check', check: { name: 'answers', result: 'works' } },
          { event: 'check', check: { name: 'uses tools', result: 'no tool call' } },
        ]);
        expect(result).toEqual({
          result: 'no tool call',
          checks: [
            { name: 'answers', result: 'works' },
            { name: 'uses tools', result: 'no tool call' },
          ],
        });
      },
    );
  });

  it('test fails, and says nothing of the lines, when one cannot be read', async () => {
    await withFakeAgent(
      () => `{"checks":["answers"]}\n${secret} not json\n{"result":"works","text":"pong"}`,
      async (handlers) => {
        const result = await handlers.test({ baseUrl: 'u', model: 'm', apiKey: secret });
        expect(result.result).toBe('failed');
        // A JSON error quotes the start of the text it could not read: a leak shows in the prefix.
        expect(JSON.stringify(result)).not.toContain(secret.slice(0, 8));
        expect(JSON.stringify(result)).not.toContain('not json');
      },
    );
  });

  it('test refuses a second Test while one runs', async () => {
    let release = (): void => undefined;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    // The agent holds the first Test's answer until `release`.
    await withFakeAgent(
      async () => {
        await held;
        return '{"result":"works","text":"pong"}';
      },
      async (handlers, lines) => {
        const input = { baseUrl: 'u', model: 'm', apiKey: 'k' };
        const first = handlers.test(input);
        await vi.waitFor(() => {
          expect(lines).toHaveLength(1);
        });
        expect(await handlers.test(input)).toEqual({
          result: 'failed',
          error: 'a Test is already running',
          checks: [],
        });
        expect(lines).toHaveLength(1);
        release();
        expect((await first).result).toBe('works');
        expect((await handlers.test(input)).result).toBe('works');
        expect(lines).toHaveLength(2);
      },
    );
  });

  it('test fails when the agent closes before the last line', async () => {
    await withFakeAgent(
      () => '{"checks":["answers"]}',
      async (handlers) => {
        expect(await handlers.test({ baseUrl: 'u', model: 'm', apiKey: 'k' })).toEqual({
          result: 'failed',
          error: 'the agent closed the socket without an answer',
          checks: [],
        });
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
