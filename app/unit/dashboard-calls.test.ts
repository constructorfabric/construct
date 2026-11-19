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
        groups: [],
        hiddenProjects: 0,
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

  it('ask dashboard-get for the document and the problem of the file', async () => {
    const problem = { kind: 'file', place: '', message: 'the file cannot be read' };
    await withFakeAgent(
      JSON.stringify({ document: null, error: problem }),
      async (handlers, lines) => {
        expect(await handlers.document()).toEqual({ ok: true, document: null, problem });
        expect(lines).toEqual(['dashboard-get']);
      },
    );
  });

  it('say so when the reply to dashboard-get is not a document', async () => {
    await withFakeAgent('{"status":"running"}', async (handlers) => {
      expect(await handlers.document()).toEqual({
        ok: false,
        error: 'the agent reply has no document',
      });
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

  it('send a project change as its command and give the document the agent answers', async () => {
    const document = {
      type: 'a type',
      title: 'Activity',
      payload: {
        period: 'today',
        sessions: 'own',
        widgets: [],
        hidden_projects: ['billing'],
        project_names: {},
      },
    };
    await withFakeAgent(JSON.stringify({ document }), async (handlers, lines) => {
      const result = await handlers.project({ action: 'hide', project: 'billing' });
      expect(result).toEqual({
        ok: true,
        problem: null,
        document: {
          title: 'Activity',
          period: 'today',
          sessions: 'own',
          widgets: [],
          hiddenProjects: ['billing'],
          projectNames: {},
        },
      });
      expect(lines).toEqual(['dashboard-hide {"project":"billing"}']);
    });
  });

  it('give the error of the agent for a change it refuses', async () => {
    await withFakeAgent('{"error":"the dashboard is not available"}', async (handlers) => {
      expect(await handlers.project({ action: 'show', project: 'billing' })).toEqual({
        ok: false,
        error: 'the dashboard is not available',
      });
    });
  });

  it('refuse a change the app does not accept, without calling the agent', async () => {
    await withFakeAgent('{}', async (handlers, lines) => {
      expect(await handlers.project({ action: 'rename', project: 'a' })).toEqual({
        ok: false,
        error: 'the window sent a project change in a form the app does not accept',
      });
      expect(lines).toEqual([]);
    });
  });
});

describe('the drill-down handlers', () => {
  it('ask dashboard-sessions for a page of a group and give its rows', async () => {
    const reply =
      '{"group":"billing","total":1,"offset":0,"limit":20,"next_offset":null,"sessions":[{"start":"2026-11-16T10:00:00+00:00","end":"2026-11-16T10:20:00+00:00","minutes":20,"tool":"claude-code","project":"billing","ticket":"No ticket","session":"abcd1234"}]}';
    await withFakeAgent(reply, async (handlers, lines) => {
      const result = await handlers.sessions({
        days: [day],
        widget: 'by-project',
        group: 'billing',
        offset: 0,
        limit: 20,
      });
      expect(result).toMatchObject({ ok: true, total: 1, nextOffset: null });
      expect(lines).toHaveLength(1);
      expect(lines[0]).toMatch(/^dashboard-sessions \{.*"group":"billing".*"widget":"by-project"/);
    });
  });

  it('ask dashboard-summary for the day and give its lines', async () => {
    const reply = '{"day":"2026-11-16","total_minutes":20,"lines":["No ticket (billing): 20 min"]}';
    await withFakeAgent(reply, async (handlers, lines) => {
      const result = await handlers.summary({ day, widget: 'end-of-day' });
      expect(result).toEqual({
        ok: true,
        lines: ['No ticket (billing): 20 min'],
      });
      expect(lines).toEqual([`dashboard-summary ${JSON.stringify({ day, widget: 'end-of-day' })}`]);
    });
  });

  it('refuse what the app does not accept, without calling the agent', async () => {
    await withFakeAgent('{}', async (handlers, lines) => {
      const refused = {
        ok: false,
        error: 'the window sent a dashboard request in a form the app does not accept',
      };
      expect(await handlers.sessions({ days: [day], widget: 'by-project' })).toEqual(refused);
      expect(await handlers.summary({ widget: 'end-of-day' })).toEqual(refused);
      expect(lines).toEqual([]);
    });
  });

  it('say so when the agent is not there', async () => {
    const handlers = createDashboardHandlers({
      env: { XDG_RUNTIME_DIR: path.join(tmpdir(), 'cb-no-such-runtime-dir') },
      platform: 'linux',
    });
    const gone = { ok: false, error: 'the agent is not running' };
    expect(
      await handlers.sessions({
        days: [day],
        widget: 'by-project',
        group: 'a',
        offset: 0,
        limit: 20,
      }),
    ).toEqual(gone);
    expect(await handlers.summary({ day, widget: 'end-of-day' })).toEqual(gone);
  });

  it('turn a reply that is not a page of sessions into an error, not a crash', async () => {
    await withFakeAgent('{"total":"many"}', async (handlers) => {
      const result = await handlers.sessions({
        days: [day],
        widget: 'by-project',
        group: 'a',
        offset: 0,
        limit: 20,
      });
      expect(result).toMatchObject({ ok: false });
    });
  });

  it('say that the agent is still reading the history, for the summary too', async () => {
    await withFakeAgent('{"error":"the activity time is being built"}', async (handlers) => {
      expect(await handlers.summary({ day, widget: 'end-of-day' })).toEqual({
        ok: false,
        error: 'the activity time is being built',
        reading: true,
      });
    });
  });
});
