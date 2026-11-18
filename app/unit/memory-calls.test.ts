import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import type { IncomingMessage, ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { createMemoryHandlers, memoryLimits } from '../src/memory-calls';
import type { MemoryLimits } from '../src/memory-calls';

const token = 'ab'.repeat(32);
const person = '11111111-6a88-4768-9dfc-6bcd5187d9ed';

function fact(id: string, property = 'skills', value: unknown = { name: id }): object {
  return {
    fact_id: id,
    entity_kind: 'skill',
    property,
    value,
    origin: { kind: 'agent' },
    stored_at: '2026-11-18T10:30:00Z',
  };
}

function page(items: object[], next: string | null, version: number | null = 7): string {
  return JSON.stringify({
    items,
    page_info: { next_cursor: next, prev_cursor: null, limit: 200 },
    version,
  });
}

interface Seen {
  readonly url: string;
  readonly authorization: string | undefined;
}

type Handler = (request: IncomingMessage, response: ServerResponse, seen: Seen[]) => void;

/** A runtime directory, a fake host on the loopback address, and the file that announces it. */
async function withHost(
  handler: Handler,
  check: (
    runtimeDir: string,
    port: number,
    seen: readonly Seen[],
    handlers: (limits?: MemoryLimits) => ReturnType<typeof createMemoryHandlers>,
  ) => Promise<void>,
): Promise<void> {
  const runtimeDir = mkdtempSync(path.join(tmpdir(), 'cb-unit-'));
  mkdirSync(path.join(runtimeDir, 'construct'));
  const seen: Seen[] = [];
  const server = createServer((request, response) => {
    seen.push({ url: request.url ?? '', authorization: request.headers.authorization });
    handler(request, response, seen);
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address() as AddressInfo;
  writeFileSync(
    path.join(runtimeDir, 'construct', 'connection.json'),
    JSON.stringify({ url: `http://127.0.0.1:${port}`, port, token }),
  );
  try {
    await check(runtimeDir, port, seen, (limits = memoryLimits) =>
      createMemoryHandlers({ env: { XDG_RUNTIME_DIR: runtimeDir }, platform: 'linux' }, limits),
    );
  } finally {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
    rmSync(runtimeDir, { recursive: true, force: true });
  }
}

const json =
  (body: string, status = 200): Handler =>
  (_request, response) => {
    response.writeHead(status, { 'Content-Type': 'application/json' });
    response.end(body);
  };

describe('the memory handlers', () => {
  it('follow the cursor over three pages and give every fact with the first version', async () => {
    const pages = new Map([
      ['', page([fact('a'), fact('b')], 'second', 7)],
      ['second', page([fact('c')], 'third', 8)],
      ['third', page([fact('d')], null, 9)],
    ]);
    await withHost(
      (request, response) => {
        const cursor = new URL(request.url ?? '', 'http://x').searchParams.get('cursor') ?? '';
        response.writeHead(200, { 'Content-Type': 'application/json' });
        response.end(pages.get(cursor));
      },
      async (_dir, _port, seen, handlers) => {
        const result = await handlers().facts();
        expect(result.ok && result.facts.map((f) => f.fact_id)).toEqual(['a', 'b', 'c', 'd']);
        expect(result.ok && result.version).toBe(7);
        expect(seen.map((s) => s.url)).toEqual([
          `/construct/v1/subjects/${person}/facts?limit=200`,
          `/construct/v1/subjects/${person}/facts?limit=200&cursor=second`,
          `/construct/v1/subjects/${person}/facts?limit=200&cursor=third`,
        ]);
        // The token goes to the host as a bearer token, and nowhere else.
        expect(seen.every((s) => s.authorization === `Bearer ${token}`)).toBe(true);
        expect(JSON.stringify(result)).not.toContain(token);
      },
    );
  });

  it('read one fact for the version', async () => {
    await withHost(json(page([fact('a')], 'more', 12)), async (_dir, _port, seen, handlers) => {
      expect(await handlers().version()).toEqual({ ok: true, version: 12 });
      expect(seen.map((s) => s.url)).toEqual([`/construct/v1/subjects/${person}/facts?limit=1`]);
    });
  });

  it('give a null version for a person without a profile', async () => {
    await withHost(json(page([], null, null)), async (_dir, _port, _seen, handlers) => {
      expect(await handlers().facts()).toEqual({ ok: true, facts: [], version: null });
    });
  });

  it('give up on a host that does not answer within the deadline', async () => {
    await withHost(
      () => undefined,
      async (_dir, _port, _seen, handlers) => {
        const started = Date.now();
        const result = await handlers({ ...memoryLimits, deadlineMs: 150 }).facts();
        expect(result).toEqual({ ok: false, error: 'Construct is not ready' });
        expect(Date.now() - started).toBeLessThan(2000);
      },
    );
  });

  it('give up when the pages together take longer than the time for a whole read', async () => {
    let count = 0;
    await withHost(
      (_request, response) => {
        count += 1;
        // Each page is within the deadline of one request, and together they are not.
        setTimeout(() => {
          response.writeHead(200, { 'Content-Type': 'application/json' });
          response.end(page([fact(`f${count}`)], `cursor-${count}`));
        }, 120);
      },
      async (_dir, _port, _seen, handlers) => {
        const started = Date.now();
        const result = await handlers({ ...memoryLimits, deadlineMs: 1000, readMs: 400 }).facts();
        expect(result).toEqual({ ok: false, error: 'Construct is not ready' });
        expect(Date.now() - started).toBeLessThan(1500);
        expect(count).toBeLessThan(10);
      },
    );
  });

  it('keep a fact whose value is nested deeper than the page draws, as text, with the rest', async () => {
    let deep: unknown = 'bottom';
    for (let level = 0; level < 12; level += 1) {
      deep = { next: deep };
    }
    await withHost(
      json(page([fact('a', 'skills', deep), fact('b')], null)),
      async (_dir, _port, _seen, handlers) => {
        const result = await handlers().facts();
        expect(result.ok && result.facts.map((f) => f.fact_id)).toEqual(['a', 'b']);
        // Eight levels of objects are kept; what is below is its JSON text.
        let value: unknown = result.ok ? result.facts[0]?.value : null;
        for (let level = 0; level < 8; level += 1) {
          value = (value as { next: unknown }).next;
        }
        expect(typeof value).toBe('string');
        expect(value).toContain('bottom');
      },
    );
  });

  it('refuse an answer that is larger than the body limit', async () => {
    await withHost(
      json(page([fact('a', 'background', 'x'.repeat(5000))], null)),
      async (_dir, _port, _seen, handlers) => {
        const result = await handlers({ ...memoryLimits, maxBodyBytes: 1024 }).facts();
        expect(result).toEqual({ ok: false, error: 'Construct is not ready' });
      },
    );
  });

  it('stop at the page limit when the pages never end', async () => {
    let count = 0;
    await withHost(
      (_request, response) => {
        count += 1;
        response.writeHead(200, { 'Content-Type': 'application/json' });
        response.end(page([fact(`f${count}`)], `cursor-${count}`));
      },
      async (_dir, _port, _seen, handlers) => {
        const result = await handlers({ ...memoryLimits, maxPages: 3 }).facts();
        expect(result).toEqual({ ok: false, error: 'Construct is not ready' });
        expect(count).toBe(3);
      },
    );
  });

  it('stop when the same cursor comes again', async () => {
    await withHost(json(page([fact('a')], 'same')), async (_dir, _port, seen, handlers) => {
      expect(await handlers().facts()).toEqual({ ok: false, error: 'Construct is not ready' });
      expect(seen).toHaveLength(2);
    });
  });

  it.each([
    ['not JSON', 'not json'],
    ['not an object', '[]'],
    ['no items', JSON.stringify({ page_info: {}, version: 1 })],
    ['an unknown category', page([{ ...fact('a'), entity_kind: 'mood' }], null)],
    ['a value that is a number', page([fact('a', 'skills', 42)], null)],
    ['a time that is not a time', page([{ ...fact('a'), stored_at: 'soon' }], null)],
    ['a version that is text', JSON.stringify({ items: [], page_info: {}, version: 'x' })],
    ['a cursor that is a number', JSON.stringify({ items: [], page_info: { next_cursor: 3 } })],
  ])('call an answer with %s a wrong shape', async (_name, body) => {
    await withHost(json(body), async (_dir, _port, _seen, handlers) => {
      expect(await handlers().facts()).toEqual({ ok: false, error: 'Construct is not ready' });
      expect(await handlers().version()).toEqual({ ok: false, error: 'Construct is not ready' });
    });
  });

  it.each([401, 404, 500, 503])('say the gear is not ready for the status %i', async (status) => {
    await withHost(json('{}', status), async (_dir, _port, _seen, handlers) => {
      expect(await handlers().facts()).toEqual({ ok: false, error: 'Construct is not ready' });
    });
  });

  it('say the agent is not running when there is no connection file', async () => {
    const runtimeDir = mkdtempSync(path.join(tmpdir(), 'cb-unit-'));
    try {
      const handlers = createMemoryHandlers({
        env: { XDG_RUNTIME_DIR: runtimeDir },
        platform: 'linux',
      });
      expect(await handlers.facts()).toEqual({ ok: false, error: 'the agent is not running' });
      expect(await handlers.version()).toEqual({ ok: false, error: 'the agent is not running' });
    } finally {
      rmSync(runtimeDir, { recursive: true, force: true });
    }
  });

  it('say the agent is not running when the host refuses the connection', async () => {
    await withHost(json('{}'), async (runtimeDir, _port, _seen, handlers) => {
      // A port nothing listens on: a server that was bound and closed.
      const closed = createServer();
      await new Promise<void>((resolve) => closed.listen(0, '127.0.0.1', resolve));
      const { port } = closed.address() as AddressInfo;
      await new Promise((resolve) => closed.close(resolve));
      writeFileSync(
        path.join(runtimeDir, 'construct', 'connection.json'),
        JSON.stringify({ port, token }),
      );
      expect(await handlers().facts()).toEqual({ ok: false, error: 'the agent is not running' });
    });
  });

  it('read the connection file again at every call', async () => {
    await withHost(json(page([fact('a')], null)), async (runtimeDir, _port, _seen, handlers) => {
      const memory = handlers();
      expect((await memory.facts()).ok).toBe(true);
      const other = createServer((_request, response) => {
        response.writeHead(200, { 'Content-Type': 'application/json' });
        response.end(page([fact('b'), fact('c')], null, 99));
      });
      await new Promise<void>((resolve) => other.listen(0, '127.0.0.1', resolve));
      const { port } = other.address() as AddressInfo;
      writeFileSync(
        path.join(runtimeDir, 'construct', 'connection.json'),
        JSON.stringify({ port, token }),
      );
      try {
        const result = await memory.facts();
        expect(result.ok && result.facts.map((f) => f.fact_id)).toEqual(['b', 'c']);
      } finally {
        await new Promise((resolve) => other.close(resolve));
      }
    });
  });

  it.each([
    ['not JSON', 'nope'],
    ['no port', JSON.stringify({ token })],
    ['a port out of range', JSON.stringify({ port: 70_000, token })],
    ['no token', JSON.stringify({ port: 8080 })],
    ['a token that is not a plain word', JSON.stringify({ port: 8080, token: 'a b\r\nc' })],
  ])('say the gear is not ready for a connection file with %s', async (_name, text) => {
    await withHost(json('{}'), async (runtimeDir, _port, _seen, handlers) => {
      writeFileSync(path.join(runtimeDir, 'construct', 'connection.json'), text);
      expect(await handlers().facts()).toEqual({ ok: false, error: 'Construct is not ready' });
    });
  });
});
