import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import type { IncomingMessage, Server, ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';
import path from 'node:path';
import { agentSocketPath } from '../src/agent-socket';
import type { Page } from '@playwright/test';
import { withStandIn } from './fixtures';
import type { Session } from './fixtures';

export const token = 'cd'.repeat(32);

export interface Fact {
  /** The id the host gives the fact; by default one that stays with it, like the gear's. */
  readonly id?: string;
  readonly entity_kind: string;
  readonly property: string;
  readonly value: unknown;
}

/** What the fake host answers, and what it has been asked. */
export interface Host {
  facts: Fact[];
  version: number | null;
  /** The status of every answer; anything but 200 is a gear that is not ready. */
  status: number;
  /** Facts in a page, so that the list takes more than one call. */
  pageSize: number;
  readonly urls: string[];
  /** The ids of the facts the page asked to delete, in order. */
  readonly deletes: string[];
  /** The status the host answers a delete with: 204 deletes the fact. */
  deleteStatus: number;
  /** While set, a delete waits for it before it is answered. */
  deleteGate: Promise<void> | null;
  /** While set, a list read (not the version look) is answered with its page only after it. */
  listGate: Promise<void> | null;
  /** How many list reads have been answered. */
  listAnswered: number;
  /** Whether collection is on, as the stand-in agent says it. */
  collecting: boolean;
  /** The stand-in agent cannot give its settings. */
  settingsFail: boolean;
  /** What the settings route says about an erasure that runs. */
  erasing: boolean;
  readonly port: () => number;
  /** Announces the host the way the agent does, in the runtime directory. */
  readonly announce: () => void;
  /** The host goes away and its file with it. */
  readonly withdraw: () => void;
  readonly stop: () => Promise<void>;
}

export function newHost(session: Session): Host {
  const file = path.join(
    path.dirname(agentSocketPath(session.env, process.platform)),
    'connection.json',
  );
  const urls: string[] = [];
  const host: Host = {
    facts: [],
    version: 1,
    status: 200,
    pageSize: 2,
    urls,
    deletes: [],
    deleteStatus: 204,
    deleteGate: null,
    listGate: null,
    listAnswered: 0,
    collecting: true,
    settingsFail: false,
    erasing: false,
    port: () => (server.address() as AddressInfo).port,
    announce: () => {
      mkdirSync(path.dirname(file), { recursive: true });
      writeFileSync(file, JSON.stringify({ port: host.port(), token }));
    },
    withdraw: () => {
      rmSync(file, { force: true });
    },
    stop: async () => {
      host.withdraw();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    },
  };
  // A fact keeps its id when others are deleted, as in the gear.
  const ids = new Map<Fact, string>();
  const idOf = (fact: Fact): string => {
    const known = ids.get(fact) ?? fact.id ?? `fact-${ids.size}`;
    ids.set(fact, known);
    return known;
  };
  const server: Server = createServer((request, response) => {
    void answer(request, response);
  });
  const answer = async (request: IncomingMessage, response: ServerResponse): Promise<void> => {
    const url = new URL(request.url ?? '', 'http://127.0.0.1');
    urls.push(`${url.pathname}${url.search}`);
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end('{}');
      return;
    }
    if (request.method === 'DELETE') {
      const id = decodeURIComponent(url.pathname.split('/').pop() ?? '');
      host.deletes.push(id);
      await host.deleteGate;
      const index = host.facts.findIndex((fact) => idOf(fact) === id);
      if (host.deleteStatus === 204 && index >= 0) {
        host.facts.splice(index, 1);
        host.version = (host.version ?? 0) + 1;
      }
      response.writeHead(host.deleteStatus).end();
      return;
    }
    if (url.pathname.endsWith('/settings')) {
      response.writeHead(host.status, { 'Content-Type': 'application/json' });
      response.end(
        JSON.stringify({
          subject_id: 's',
          personalization_enabled: host.collecting,
          erasure_in_progress: host.erasing,
        }),
      );
      return;
    }
    if (host.status !== 200) {
      response.writeHead(host.status).end('{}');
      return;
    }
    const limit = Math.min(Number(url.searchParams.get('limit') ?? '50'), host.pageSize);
    const from = Number(url.searchParams.get('cursor') ?? '0');
    const items = host.facts.slice(from, from + limit).map((listed) => ({
      entity_kind: listed.entity_kind,
      property: listed.property,
      value: listed.value,
      fact_id: idOf(listed),
      origin: { kind: 'agent' },
      stored_at: '2026-11-18T10:30:00Z',
    }));
    const next = from + limit < host.facts.length ? String(from + limit) : null;
    const version = host.version;
    // The page is as it is now; a gated read tells it later, after what changed meanwhile.
    if (limit > 1) {
      await host.listGate;
      host.listAnswered += 1;
    }
    response.writeHead(200, { 'Content-Type': 'application/json' });
    response.end(
      JSON.stringify({
        items,
        page_info: { next_cursor: next, prev_cursor: null, limit },
        version,
      }),
    );
  };
  server.listen(0, '127.0.0.1');
  return host;
}

/**
 * Puts a stand-in agent that answers with `answer` on the agent's socket and runs `check` on a
 * new window, with `host` stopped at the end. The host is not announced: the check does that
 * when it wants the gear to be there.
 */
export async function withHostAndAgent(
  session: Session,
  host: Host,
  answer: (line: string) => string | Promise<string>,
  check: (page: Page) => Promise<void>,
): Promise<void> {
  try {
    await withStandIn(session, answer, async (page) => {
      // The real agent is stopped by now and has taken its connection file with it.
      host.withdraw();
      await check(page);
    });
  } finally {
    await host.stop();
  }
}

export const openMemory = (page: Page) => page.getByRole('button', { name: 'Memory', exact: true });
