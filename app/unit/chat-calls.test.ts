import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import net from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { createChatHandlers } from '../src/chat-calls';
import type { ChatEvent } from '../src/chat-protocol';

const id = '3f2a9c1e-0b7d-4c55-9a10-6d2b8e4f7a01';
const secretMessage = 'my-private-question-71c3';
const limits = {
  stream: { idleMs: 400, maxLineChars: 1024, maxTotalChars: 8192 },
  stopGraceMs: 300,
};

/** Waits until `done()` holds, for at most 3 s. */
async function until(done: () => boolean, what: string): Promise<void> {
  const deadline = Date.now() + 3000;
  while (!done()) {
    if (Date.now() > deadline) {
      throw new Error(`timed out waiting for ${what}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

type Behaviour = (line: string, client: net.Socket) => void;

// A fake agent: `behave` decides what it does with each line, and `lines` has what it was sent.
async function withFakeAgent(
  behave: Behaviour,
  check: (
    handlers: ReturnType<typeof createChatHandlers>,
    events: ChatEvent[],
    lines: string[],
    ended: () => Promise<void>,
  ) => Promise<void>,
  stopGraceMs = limits.stopGraceMs,
): Promise<void> {
  const runtimeDir = mkdtempSync(path.join(tmpdir(), 'cb-unit-'));
  mkdirSync(path.join(runtimeDir, 'construct'));
  const lines: string[] = [];
  const events: ChatEvent[] = [];
  const clients = new Set<net.Socket>();
  const server = net.createServer((client) => {
    clients.add(client);
    client.setEncoding('utf8');
    client.once('data', (data: string) => {
      const line = data.trim();
      lines.push(line);
      behave(line, client);
    });
    client.on('error', () => undefined);
  });
  await new Promise<void>((resolve) => {
    server.listen(path.join(runtimeDir, 'construct', 'agent.sock'), resolve);
  });
  const handlers = createChatHandlers(
    { env: { XDG_RUNTIME_DIR: runtimeDir }, platform: 'linux' },
    (event) => events.push(event),
    { stream: limits.stream, stopGraceMs },
  );
  const ended = (): Promise<void> =>
    until(() => events.some((e) => e.event === 'done' || e.event === 'error'), 'the answer to end');
  try {
    await check(handlers, events, lines, ended);
  } finally {
    handlers.close();
    clients.forEach((client) => {
      client.destroy();
    });
    await new Promise((resolve) => server.close(resolve));
    rmSync(runtimeDir, { recursive: true, force: true });
  }
}

const answering: Behaviour = (_line, client) => {
  client.write(`{"event":"start","conversation":"${id}"}\n`);
  // An event the window does not know is skipped, not an error.
  client.write('{"event":"delta","text":"Hel"}\n{"event":"tool","line":"Read notes.txt"}\n');
  client.write('{"event":"other"}\n');
  client.write('{"event":"delta","text":"lo"}\n');
  client.end('{"event":"done"}\n');
};

describe('the chat handlers', () => {
  it('forwards each line of an answer as an event, in order, and skips an unknown event', async () => {
    await withFakeAgent(answering, async (handlers, events, lines, ended) => {
      expect(handlers.send({ conversation: '', message: secretMessage })).toEqual({ ok: true });
      await ended();
      expect(events).toEqual([
        { event: 'start', conversation: id },
        { event: 'delta', text: 'Hel' },
        { event: 'tool', line: 'Read notes.txt' },
        { event: 'delta', text: 'lo' },
        { event: 'done', stopped: false },
      ]);
      expect(JSON.parse(lines[0]?.slice('chat-send '.length) ?? '')).toEqual({
        conversation: '',
        message: secretMessage,
      });
    });
  });

  it('refuses a second answer while one runs, and a bad request at once', async () => {
    await withFakeAgent(
      () => undefined,
      (handlers) => {
        expect(handlers.send({ conversation: 'all', message: 'x' })).toEqual({
          ok: false,
          error: 'the window sent a chat request in a form the app does not accept',
        });
        expect(handlers.send({ conversation: '', message: 'x' })).toEqual({ ok: true });
        expect(handlers.send({ conversation: '', message: 'y' })).toEqual({
          ok: false,
          error: 'busy',
        });
        return Promise.resolve();
      },
    );
  });

  it('turns a socket that closes before the end into the reason cut', async () => {
    await withFakeAgent(
      (_line, client) => {
        client.end('{"event":"delta","text":"Hel"}\n');
      },
      async (handlers, events, _lines, ended) => {
        handlers.send({ conversation: '', message: 'x' });
        await ended();
        expect(events.at(-1)).toEqual({ event: 'error', reason: 'cut' });
      },
    );
  });

  it('turns a silent agent into a timeout', async () => {
    await withFakeAgent(
      () => undefined,
      async (handlers, events, _lines, ended) => {
        handlers.send({ conversation: '', message: 'x' });
        await ended();
        expect(events).toEqual([{ event: 'error', reason: 'timeout' }]);
      },
    );
  });

  it('ends an answer with a line that is too long', async () => {
    await withFakeAgent(
      (_line, client) => {
        client.write(`{"event":"delta","text":"${'a'.repeat(2000)}"}\n`);
      },
      async (handlers, events, _lines, ended) => {
        handlers.send({ conversation: '', message: 'x' });
        await ended();
        expect(events).toEqual([{ event: 'error', reason: 'cut' }]);
      },
    );
  });

  it('does not take a last line without its newline for a line', async () => {
    await withFakeAgent(
      (_line, client) => {
        client.end('{"event":"delta","text":"Hel"}\n{"event":"done"}');
      },
      async (handlers, events, _lines, ended) => {
        handlers.send({ conversation: '', message: 'x' });
        await ended();
        expect(events).toEqual([
          { event: 'delta', text: 'Hel' },
          { event: 'error', reason: 'cut' },
        ]);
      },
    );
  });

  it('ends an answer whose unfinished line is already past the line cap', async () => {
    await withFakeAgent(
      (_line, client) => {
        client.write(`{"event":"delta","text":"${'a'.repeat(2000)}`);
      },
      async (handlers, events, _lines, ended) => {
        handlers.send({ conversation: '', message: 'x' });
        await ended();
        expect(events).toEqual([{ event: 'error', reason: 'cut' }]);
      },
    );
  });

  it('ends an answer that grows past the total cap', async () => {
    await withFakeAgent(
      (_line, client) => {
        for (let i = 0; i < 20; i += 1) {
          client.write(`{"event":"delta","text":"${'a'.repeat(900)}"}\n`);
        }
      },
      async (handlers, events, _lines, ended) => {
        handlers.send({ conversation: '', message: 'x' });
        await ended();
        expect(events.at(-1)).toEqual({ event: 'error', reason: 'cut' });
      },
    );
  });

  it('reports a line it cannot read as an unreadable answer, without the line', async () => {
    await withFakeAgent(
      (_line, client) => {
        client.write(`not json ${secretMessage}\n`);
      },
      async (handlers, events, _lines, ended) => {
        handlers.send({ conversation: '', message: 'x' });
        await ended();
        expect(events).toEqual([{ event: 'error', reason: 'unreadable answer' }]);
        expect(JSON.stringify(events)).not.toContain(secretMessage);
      },
    );
  });

  it('Stop sends chat-stop and ends with the agent’s own done', async () => {
    const clientByRole: { send?: net.Socket } = {};
    await withFakeAgent(
      (line, client) => {
        if (line.startsWith('chat-send')) {
          client.write(`{"event":"start","conversation":"${id}"}\n`);
          client.write('{"event":"delta","text":"Hel"}\n');
          clientByRole.send = client;
        } else if (line.startsWith('chat-stop')) {
          client.end('{"stopped":true}\n');
          clientByRole.send?.end('{"event":"done","stopped":true}\n');
        }
      },
      async (handlers, events, lines, ended) => {
        handlers.send({ conversation: '', message: 'x' });
        await until(() => events.some((e) => e.event === 'delta'), 'the first piece');
        await handlers.stop();
        await ended();
        expect(lines).toContain(`chat-stop ${id}`);
        expect(events.at(-1)).toEqual({ event: 'done', stopped: true });
      },
      // A grace period far longer than the test waits: the answer can only end through the agent's
      // own `done`, and `ended()` times out when the app ignores it.
      60_000,
    );
  });

  it('Stop ends the answer itself, and closes the socket, when the agent does not', async () => {
    let sendClosed = false;
    await withFakeAgent(
      (line, client) => {
        if (line.startsWith('chat-send')) {
          client.write(`{"event":"start","conversation":"${id}"}\n`);
          client.on('close', () => {
            sendClosed = true;
          });
        } else {
          client.end('{"stopped":false}\n');
        }
      },
      async (handlers, events, _lines, ended) => {
        handlers.send({ conversation: '', message: 'x' });
        await until(() => events.length > 0, 'the start');
        await handlers.stop();
        await ended();
        expect(events.at(-1)).toEqual({ event: 'done', stopped: true });
        // The app let go of the connection, which the agent takes as a stop too.
        await until(() => sendClosed, 'the socket to close');
        // The next answer can start.
        expect(handlers.send({ conversation: '', message: 'y' })).toEqual({ ok: true });
      },
    );
  });

  it('lists conversations, reads one without its tool records, and deletes', async () => {
    await withFakeAgent(
      (line, client) => {
        if (line === 'chat-list') {
          client.end(`{"conversations":[{"conversation":"${id}","title":"hi","time":"t"}]}\n`);
        } else if (line.startsWith('chat-get')) {
          client.end(
            '{"kind":"turn","role":"user","time":"1","text":"hi"}\n' +
              '{"kind":"toolcall","time":"2","call_id":"c","name":"n","arguments":"{}"}\n' +
              '{"kind":"turn","role":"assistant","time":"3","text":"yo"}\n',
          );
        } else {
          client.end('{"deleted":1}\n');
        }
      },
      async (handlers, _events, lines) => {
        expect(await handlers.list()).toEqual({
          ok: true,
          conversations: [{ id, title: 'hi', time: 't' }],
        });
        expect(await handlers.get(id)).toEqual({
          ok: true,
          messages: [
            { role: 'user', text: 'hi' },
            { role: 'assistant', text: 'yo' },
          ],
        });
        expect(await handlers.remove(id)).toEqual({ ok: true });
        expect(await handlers.remove('all')).toEqual({ ok: true });
        expect(lines).toEqual([
          'chat-list',
          `chat-get ${id}`,
          `chat-delete ${id}`,
          'chat-delete all',
        ]);
      },
    );
  });

  it('never puts a window-made string into a command line', async () => {
    await withFakeAgent(
      () => undefined,
      async (handlers, _events, lines) => {
        const refused = {
          ok: false,
          error: 'the window sent a chat request in a form the app does not accept',
        };
        expect(await handlers.get(`${id}\nchat-delete all`)).toEqual(refused);
        expect(await handlers.remove('all\nstatus')).toEqual(refused);
        expect(lines).toEqual([]);
      },
    );
  });

  it('keeps what the agent answered out of the error text, for each call', async () => {
    const reply = `not json ${secretMessage}`;
    await withFakeAgent(
      (_line, client) => {
        client.end(`${reply}\n`);
      },
      async (handlers) => {
        const results = [await handlers.list(), await handlers.get(id), await handlers.remove(id)];
        for (const result of results) {
          expect(result).toEqual({
            ok: false,
            error: 'the agent gave an answer the app cannot read',
          });
          expect(JSON.stringify(result)).not.toContain(secretMessage);
        }
      },
    );
  });
});

describe('the chat handlers with no agent', () => {
  it('say the agent is not running, and a send reports it as an event', async () => {
    const runtimeDir = mkdtempSync(path.join(tmpdir(), 'cb-unit-'));
    mkdirSync(path.join(runtimeDir, 'construct'));
    const events: ChatEvent[] = [];
    const handlers = createChatHandlers(
      { env: { XDG_RUNTIME_DIR: runtimeDir }, platform: 'linux' },
      (event) => events.push(event),
      limits,
    );
    try {
      const absent = { ok: false, error: 'the agent is not running' };
      expect(await handlers.list()).toEqual(absent);
      expect(await handlers.get(id)).toEqual(absent);
      expect(await handlers.remove(id)).toEqual(absent);
      expect(handlers.send({ conversation: '', message: 'x' })).toEqual({ ok: true });
      await until(() => events.length > 0, 'the event');
      expect(events).toEqual([{ event: 'error', reason: 'agent not running' }]);
      // The slot is free again.
      expect(handlers.send({ conversation: '', message: 'y' })).toEqual({ ok: true });
    } finally {
      handlers.close();
      rmSync(runtimeDir, { recursive: true, force: true });
    }
  });

  it('refuse a runtime directory that is not set', async () => {
    const handlers = createChatHandlers({ env: {}, platform: 'linux' }, () => undefined, limits);
    const unset = { ok: false, error: 'XDG_RUNTIME_DIR is not set' };
    expect(await handlers.list()).toEqual(unset);
    expect(await handlers.get(id)).toEqual(unset);
    expect(await handlers.remove(id)).toEqual(unset);
    expect(handlers.send({ conversation: '', message: 'x' })).toEqual(unset);
  });
});
