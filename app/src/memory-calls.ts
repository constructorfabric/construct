import { readFile } from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import { isRecord } from './agent-json';
import { agentSocketPath, callAgent, failureText } from './agent-socket';
import type { CallLimits, Where } from './agent-socket';
import {
  eraseLine,
  isDeletableFactId,
  parseEraseReply,
  parseErasing,
  parseFactsPage,
  reasonAgentNotRunning,
  reasonNotReady,
} from './memory-protocol';
import type {
  EraseResult,
  ErasingResult,
  FactDeleteResult,
  FactsResult,
  MemoryFact,
  VersionResult,
} from './memory-protocol';

/** The person the agent's host serves: the one identity of a desktop. */
const personId = '11111111-6a88-4768-9dfc-6bcd5187d9ed';
const connectionFile = 'connection.json';
/** The gear cuts a larger page to this. */
const pageSize = 200;

/**
 * How the calls to the gear are bounded: the wait for one answer, the time for a whole list over
 * its pages, an answer's size, and the pages followed.
 */
export interface MemoryLimits {
  readonly deadlineMs: number;
  readonly readMs: number;
  readonly maxBodyBytes: number;
  readonly maxPages: number;
}

export const memoryLimits: MemoryLimits = {
  deadlineMs: 5000,
  readMs: 30_000,
  maxBodyBytes: 2 * 1024 * 1024,
  maxPages: 50,
};

/**
 * `memory-erase` waits for what the agent holds (a running answer up to 30 s, the setup run up to
 * 60 s) and makes two calls of up to 15 s to the host, so it takes about 120 s when nothing else
 * holds the agent. The app waits 3 minutes; an answer later than that is shown as a failure
 * with the app's own reason, and the page then looks at the gear.
 */
export const eraseLimits: CallLimits = { deadlineMs: 180_000, maxReplyChars: 4096 };

export interface MemoryHandlers {
  readonly facts: () => Promise<FactsResult>;
  readonly version: () => Promise<VersionResult>;
  /** The id comes from the window, so it is checked here before it is put in a route. */
  readonly remove: (factId: unknown) => Promise<FactDeleteResult>;
  /** Has the agent erase everything about the person; the end is read with `erasing`. */
  readonly erase: () => Promise<EraseResult>;
  /** Whether the gear still reports an erasure that runs. */
  readonly erasing: () => Promise<ErasingResult>;
}

/** A failure whose text is one of the app's fixed reasons: it never holds a token or an answer. */
class MemoryFailure extends Error {}

const notRunning = (): MemoryFailure => new MemoryFailure(reasonAgentNotRunning);
const notReady = (): MemoryFailure => new MemoryFailure(reasonNotReady);

interface Connection {
  readonly port: number;
  readonly token: string;
}

const tokenShape = /^[\w.~+/=-]{1,256}$/;

/**
 * What the agent's host announced: its port and token, read on every call, since the host can
 * come back on another port. No file is an agent that is not running; a file that says nothing
 * usable is a gear that is not ready.
 */
async function readConnection(where: Where): Promise<Connection> {
  let file: string;
  try {
    file = path.join(path.dirname(agentSocketPath(where.env, where.platform)), connectionFile);
  } catch {
    throw notRunning();
  }
  let text: string;
  try {
    text = await readFile(file, 'utf8');
  } catch {
    throw notRunning();
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    throw notReady();
  }
  if (!isRecord(parsed)) {
    throw notReady();
  }
  const { port, token } = parsed;
  if (
    typeof port !== 'number' ||
    !Number.isInteger(port) ||
    port < 1 ||
    port > 65_535 ||
    typeof token !== 'string' ||
    !tokenShape.test(token)
  ) {
    throw notReady();
  }
  return { port, token };
}

/**
 * One request to the host on the loopback address, with the token. It succeeds on a status in
 * `accepted`: with the JSON body when `readBody` is set, else without reading the body. Any other
 * status, or a body that is too large or not JSON, is a gear that is not ready.
 */
function send(
  connection: Connection,
  method: 'GET' | 'DELETE',
  route: string,
  limits: MemoryLimits,
  accepted: readonly number[],
  readBody: boolean,
): Promise<unknown> {
  return new Promise((resolve, reject) => {
    let settled = false;
    const settle = (finish: () => void): void => {
      if (!settled) {
        settled = true;
        clearTimeout(deadline);
        finish();
      }
    };
    const request = http.request(
      {
        host: '127.0.0.1',
        port: connection.port,
        path: route,
        method,
        headers: { Authorization: `Bearer ${connection.token}`, Accept: 'application/json' },
      },
      (response) => {
        if (!accepted.includes(response.statusCode ?? 0)) {
          response.resume();
          settle(() => {
            request.destroy();
            reject(notReady());
          });
          return;
        }
        if (!readBody) {
          response.resume();
          settle(() => {
            request.destroy();
            resolve(undefined);
          });
          return;
        }
        const chunks: Buffer[] = [];
        let size = 0;
        response.on('data', (chunk: Buffer) => {
          size += chunk.length;
          if (size > limits.maxBodyBytes) {
            settle(() => {
              request.destroy();
              reject(notReady());
            });
            return;
          }
          chunks.push(chunk);
        });
        response.on('end', () => {
          settle(() => {
            try {
              resolve(JSON.parse(Buffer.concat(chunks).toString('utf8')));
            } catch {
              reject(notReady());
            }
          });
        });
        response.on('error', () => {
          settle(() => {
            reject(notReady());
          });
        });
      },
    );
    // The deadline is for the whole request, whatever the host writes meanwhile.
    const deadline = setTimeout(() => {
      settle(() => {
        request.destroy();
        reject(notReady());
      });
    }, limits.deadlineMs);
    request.on('error', (error: NodeJS.ErrnoException) => {
      settle(() => {
        reject(error.code === 'ECONNREFUSED' ? notRunning() : notReady());
      });
    });
    request.end();
  });
}

function subjectRoute(tail: string): string {
  return `/construct/v1/subjects/${personId}/${tail}`;
}

function factsRoute(query: string): string {
  return subjectRoute(`facts?${query}`);
}

async function readPage(
  where: Where,
  query: string,
  limits: MemoryLimits,
): Promise<ReturnType<typeof parseFactsPage>> {
  // The file is read again for every page: the host may have come back in between.
  const connection = await readConnection(where);
  return parseFactsPage(await send(connection, 'GET', factsRoute(query), limits, [200], true));
}

function failure(error: unknown): { readonly ok: false; readonly error: string } {
  return { ok: false, error: error instanceof MemoryFailure ? error.message : reasonNotReady };
}

/**
 * The calls of the Memory page to the gear's own routes for the person. The window asks for
 * "the facts", "the version" and "delete this fact"; the port and the token stay here, and every failure
 * is answered as a value with a fixed reason.
 */
export function createMemoryHandlers(
  where: Where,
  limits: MemoryLimits = memoryLimits,
): MemoryHandlers {
  return {
    facts: async () => {
      try {
        const facts: MemoryFact[] = [];
        let version: number | null = null;
        let cursor: string | null = null;
        const seen = new Set<string>();
        const readUntil = Date.now() + limits.readMs;
        for (let pages = 0; ; pages += 1) {
          // The pages together have `readMs`; a request has what is left of it, or `deadlineMs`.
          const left = readUntil - Date.now();
          if (pages >= limits.maxPages || left <= 0) {
            throw notReady();
          }
          const pageLimits = { ...limits, deadlineMs: Math.min(limits.deadlineMs, left) };
          const query: string =
            cursor === null
              ? `limit=${pageSize}`
              : `limit=${pageSize}&cursor=${encodeURIComponent(cursor)}`;
          const page = await readPage(where, query, pageLimits);
          if (page === null) {
            throw notReady();
          }
          // The version of the first page: a change while the pages are read shows at the next look.
          if (pages === 0) {
            version = page.version;
          }
          facts.push(...page.facts);
          if (page.nextCursor === null) {
            return { ok: true, facts, version };
          }
          // A cursor that comes again would never end.
          if (seen.has(page.nextCursor)) {
            throw notReady();
          }
          seen.add(page.nextCursor);
          cursor = page.nextCursor;
        }
      } catch (error) {
        return failure(error);
      }
    },
    remove: async (factId) => {
      try {
        if (typeof factId !== 'string' || !isDeletableFactId(factId)) {
          throw notReady();
        }
        const connection = await readConnection(where);
        // A 204 and a 404 both mean the fact is gone; the body is never read.
        await send(
          connection,
          'DELETE',
          subjectRoute(`facts/${encodeURIComponent(factId)}`),
          limits,
          [204, 404],
          false,
        );
        return { ok: true };
      } catch (error) {
        return failure(error);
      }
    },
    erase: async () => {
      try {
        return parseEraseReply(await callAgent(where.env, where.platform, eraseLine, eraseLimits));
      } catch (error) {
        return { ok: false, error: failureText(error) };
      }
    },
    erasing: async () => {
      try {
        const connection = await readConnection(where);
        const erasing = parseErasing(
          await send(connection, 'GET', subjectRoute('settings'), limits, [200], true),
        );
        if (erasing === null) {
          throw notReady();
        }
        return { ok: true, erasing };
      } catch (error) {
        return failure(error);
      }
    },
    version: async () => {
      try {
        const page = await readPage(where, 'limit=1', limits);
        if (page === null) {
          throw notReady();
        }
        return { ok: true, version: page.version };
      } catch (error) {
        return failure(error);
      }
    },
  };
}
