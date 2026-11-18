import { createServer } from 'node:http';
import type { IncomingMessage, Server, ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';

export const goodKey = 'sk-fake-7d41c9e0a5b2';
export const goodModel = 'fake-model';

/** What the fake provider streams for one request: pieces, a pause between them, and a wait. */
export interface Script {
  readonly chunks: readonly string[];
  readonly pauseMs: number;
  /** Before this piece the provider waits for `release()`, or for ever when `forever`. */
  readonly holdAt?: number;
  readonly forever?: boolean;
}

/** A tool call the model makes: the tool's name and the arguments as an object. */
export interface ToolCall {
  readonly name: string;
  readonly arguments: Record<string, unknown>;
}

export interface Seen {
  readonly authorization: string | undefined;
  readonly model: string;
}

export interface Provider {
  /** `http://127.0.0.1:<port>/v1` */
  readonly url: string;
  /** What each request carried, in order. */
  readonly seen: readonly Seen[];
  /** A plain (not streamed) answer is held back this long, so a test can look at the window. */
  delayMs: number;
  /** A model that answers plain text where a request offers tools, and never calls one. */
  ignoresTools: boolean;
  /** What a streamed answer is made of. */
  script: Script;
  /** Tool calls for the next streamed requests, one call for each: a request takes the first. */
  calls: ToolCall[];
  /** How many pieces the provider has sent, over all requests. */
  readonly sent: () => number;
  /** Streamed requests the client closed before the provider was done. */
  readonly aborted: () => number;
  /** Lets a streamed answer go on from its `holdAt`. */
  readonly release: () => void;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function readBody(request: IncomingMessage): Promise<string> {
  return new Promise((resolve) => {
    let body = '';
    request.on('data', (chunk: Buffer) => {
      body += chunk.toString();
    });
    request.on('end', () => {
      resolve(body);
    });
  });
}

/** What a model that uses tools answers: the call of `add`, then, with its result, the sum. */
function toolTurn(messages: unknown): Record<string, unknown> {
  const answered = Array.isArray(messages) && messages.some((m) => Object(m).role === 'tool');
  if (answered) {
    return { role: 'assistant', content: 'The sum is 5.' };
  }
  return {
    role: 'assistant',
    content: null,
    tool_calls: [
      {
        id: 'call_add',
        type: 'function',
        function: { name: 'add', arguments: JSON.stringify({ a: 2, b: 3 }) },
      },
    ],
  };
}

/**
 * A fake provider on a local port: it accepts one key and one model, like the real one would, and
 * answers a request with `stream: true` in pieces and any other with one message. A refusal
 * repeats the request's key and body, as a careless provider would, so a test can tell whether the
 * agent or the app let such text through.
 */
export async function startProvider(): Promise<{ provider: Provider; server: Server }> {
  const seen: Seen[] = [];
  let sent = 0;
  let aborted = 0;
  let open = (): void => undefined;
  const state = { gate: new Promise<void>(() => undefined) };
  const newGate = (): void => {
    state.gate = new Promise<void>((resolve) => {
      open = resolve;
    });
  };
  newGate();
  const provider: Provider = {
    url: '',
    seen,
    delayMs: 0,
    ignoresTools: false,
    script: { chunks: ['pong'], pauseMs: 0 },
    calls: [],
    sent: () => sent,
    aborted: () => aborted,
    release: () => {
      open();
    },
  };

  const stream = async (response: ServerResponse, script: Script): Promise<void> => {
    let closed = false;
    response.on('close', () => {
      if (!response.writableEnded) {
        closed = true;
        aborted += 1;
      }
    });
    response.writeHead(200, { 'Content-Type': 'text/event-stream' });
    for (const [index, content] of script.chunks.entries()) {
      if (index === script.holdAt) {
        await (script.forever === true ? new Promise<void>(() => undefined) : state.gate);
      } else if (index > 0) {
        await sleep(script.pauseMs);
      }
      if (closed) {
        return;
      }
      response.write(`data: ${JSON.stringify({ choices: [{ delta: { content } }] })}\n\n`);
      sent += 1;
    }
    response.end('data: [DONE]\n\n');
  };

  const streamCall = (response: ServerResponse, call: ToolCall): void => {
    response.writeHead(200, { 'Content-Type': 'text/event-stream' });
    const delta = {
      tool_calls: [
        {
          index: 0,
          id: `call_${String(seen.length)}`,
          type: 'function',
          function: { name: call.name, arguments: JSON.stringify(call.arguments) },
        },
      ],
    };
    response.write(`data: ${JSON.stringify({ choices: [{ delta }] })}\n\n`);
    const finish = { choices: [{ delta: {}, finish_reason: 'tool_calls' }] };
    response.write(`data: ${JSON.stringify(finish)}\n\n`);
    response.end('data: [DONE]\n\n');
  };

  const server = createServer((request, response) => {
    void readBody(request).then((raw) => {
      const body = Object(JSON.parse(raw) as unknown) as Record<string, unknown>;
      const model = String(body.model);
      const { authorization } = request.headers;
      seen.push({ authorization, model });
      const answer = (status: number, payload: unknown): void => {
        setTimeout(() => {
          response.writeHead(status, { 'Content-Type': 'application/json' });
          response.end(JSON.stringify(payload));
        }, provider.delayMs);
      };
      if (authorization !== `Bearer ${goodKey}`) {
        answer(401, { error: { message: `bad key ${String(authorization)} in ${raw}` } });
      } else if (model !== goodModel) {
        answer(404, { error: { message: `no such model, request ${raw}` } });
      } else if (body.stream === true) {
        newGate();
        const call = provider.calls.shift();
        if (call === undefined) {
          void stream(response, provider.script);
        } else {
          streamCall(response, call);
        }
      } else if (Array.isArray(body.tools) && !provider.ignoresTools) {
        answer(200, { choices: [{ message: toolTurn(body.messages) }] });
      } else {
        answer(200, { choices: [{ message: { role: 'assistant', content: 'pong' } }] });
      }
    });
  });
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', resolve);
  });
  const { port } = server.address() as AddressInfo;
  return { provider: Object.assign(provider, { url: `http://127.0.0.1:${port}/v1` }), server };
}

/** A local port with nothing behind it. */
export async function closedPort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', resolve);
  });
  const { port } = server.address() as AddressInfo;
  await new Promise((resolve) => server.close(resolve));
  return port;
}

/** The `provider` fixture: a fresh fake provider for each test. */
export const providerFixture = {
  // eslint-disable-next-line no-empty-pattern
  provider: async ({}: object, use: (provider: Provider) => Promise<void>): Promise<void> => {
    const { provider, server } = await startProvider();
    await use(provider);
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  },
};
