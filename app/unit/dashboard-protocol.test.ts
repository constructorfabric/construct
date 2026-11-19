import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  dashboardNumbersLine,
  dashboardSessionsLine,
  dashboardSummaryLine,
  maxDays,
  parseDashboardInput,
  parseDashboardReply,
  parseDocumentReply,
  parseProjectChange,
  parseSessionsInput,
  parseSessionsReply,
  parseSummaryInput,
  parseSummaryReply,
  projectChangeLine,
  readingHistoryError,
} from '../src/dashboard-protocol';

const day = { label: '2026-11-16', start: '2026-11-16T00:00:00Z', end: '2026-11-17T00:00:00Z' };

describe('parseDashboardInput', () => {
  it('takes the days and whose time, and nothing else', () => {
    expect(parseDashboardInput({ days: [day], sessions: 'own', extra: 1 })).toEqual({
      days: [day],
      sessions: 'own',
    });
  });

  it('takes a widget with the days, and refuses a widget that is named with the counting', () => {
    expect(parseDashboardInput({ days: [day], widget: 'by-project' })).toEqual({
      days: [day],
      widget: 'by-project',
    });
    expect(parseDashboardInput({ days: [day], widget: 'by-project', sessions: 'own' })).toBeNull();
    expect(parseDashboardInput({ days: [day], widget: 'By Project' })).toBeNull();
    expect(parseDashboardInput({ days: [day], widget: 7 })).toBeNull();
  });

  it('takes a filter of lists of names, and only those', () => {
    expect(
      parseDashboardInput({
        days: [day],
        sessions: 'own',
        filter: { projects: ['a'], tools: [], other: ['x'] },
      }),
    ).toEqual({ days: [day], sessions: 'own', filter: { projects: ['a'], tools: [] } });
    expect(
      parseDashboardInput({ days: [day], sessions: 'own', filter: { projects: [1] } }),
    ).toBeNull();
    expect(parseDashboardInput({ days: [day], sessions: 'own', filter: 'a' })).toBeNull();
  });

  it.each([
    ['not an object', 'x'],
    ['no days', { days: [], sessions: 'own' }],
    ['days that are not a list', { days: day, sessions: 'own' }],
    ['an unknown choice', { days: [day], sessions: 'everyone' }],
    ['a day without a label', { days: [{ ...day, label: '' }], sessions: 'own' }],
    ['a time with seconds', { days: [{ ...day, start: '2026-11-16T00:00:30Z' }], sessions: 'own' }],
    [
      'a time that is not UTC',
      { days: [{ ...day, end: '2026-11-17T00:00:00+02:00' }], sessions: 'own' },
    ],
    ['too many days', { days: Array.from({ length: maxDays + 1 }, () => day), sessions: 'own' }],
  ])('refuses %s', (_name, input) => {
    expect(parseDashboardInput(input)).toBeNull();
  });
});

describe('dashboardNumbersLine', () => {
  it('asks for a widget by its id, with the days only', () => {
    const line = dashboardNumbersLine({ days: [day], widget: 'total' });
    expect(JSON.parse(line.slice('dashboard-numbers '.length))).toEqual({
      days: [day],
      widget: 'total',
    });
  });

  it('is the command and a JSON object with the days and the choice', () => {
    const line = dashboardNumbersLine({ days: [day], sessions: 'all' });
    expect(line.startsWith('dashboard-numbers ')).toBe(true);
    expect(JSON.parse(line.slice('dashboard-numbers '.length))).toEqual({
      days: [day],
      sessions: 'all',
    });
  });
});

describe('parseDashboardReply', () => {
  it('keeps the totals, the minutes of each day and the groups', () => {
    const reply = JSON.stringify({
      total_minutes: 95.5,
      session_count: 3,
      hidden_project_count: 0,
      groups: [{ key: 'a project', minutes: 95.5, share: 1, session_count: 3 }],
      days: [{ label: '2026-11-16', minutes: 95.5 }],
    });
    expect(parseDashboardReply(reply)).toEqual({
      ok: true,
      totalMinutes: 95.5,
      sessionCount: 3,
      days: [{ label: '2026-11-16', minutes: 95.5 }],
      groups: [{ key: 'a project', minutes: 95.5, share: 1, sessions: 3 }],
      hiddenProjects: 0,
    });
  });

  it('keeps how many hidden projects have time in the days', () => {
    const reply = JSON.stringify({
      total_minutes: 0,
      session_count: 0,
      hidden_project_count: 2,
      groups: [],
      days: [],
    });
    expect(parseDashboardReply(reply)).toMatchObject({ ok: true, hiddenProjects: 2 });
  });

  it('gives the text of an error', () => {
    expect(parseDashboardReply('{"error":"the dashboard numbers are not available"}')).toEqual({
      ok: false,
      error: 'the dashboard numbers are not available',
    });
  });

  it('says the agent is still reading the history, apart from other errors', () => {
    expect(parseDashboardReply(JSON.stringify({ error: readingHistoryError }))).toEqual({
      ok: false,
      error: readingHistoryError,
      reading: true,
    });
  });

  it('knows the sentence the agent gives while it reads the history', () => {
    // The page tells that state from a failure by the agent's words, so the agent's source is read.
    const feed = readFileSync(new URL('../../agent/src/activity/feed.rs', import.meta.url), 'utf8');
    expect(feed).toContain(`"${readingHistoryError}"`);
  });

  it('throws on a reply that has no numbers', () => {
    // The count of hidden projects is part of the numbers: a reply without it is not accepted.
    expect(() =>
      parseDashboardReply('{"total_minutes":1,"session_count":1,"days":[],"groups":[]}'),
    ).toThrow('no numbers');
    expect(() => parseDashboardReply('{"status":"running"}')).toThrow('no numbers');
    expect(() =>
      parseDashboardReply(
        '{"total_minutes":1,"session_count":1,"hidden_project_count":0,"days":[{}],"groups":[]}',
      ),
    ).toThrow('a day');
    expect(() =>
      parseDashboardReply(
        '{"total_minutes":1,"session_count":1,"hidden_project_count":0,"days":[],"groups":[{"key":"a"}]}',
      ),
    ).toThrow('a group');
  });
});

describe('parseDocumentReply', () => {
  const document = {
    type: 'a type',
    title: 'Activity',
    payload: {
      period: 'this_week',
      sessions: 'own',
      widgets: [
        {
          id: 'by-project',
          kind: 'table',
          group_by: 'project',
          period: 'last_week',
          sessions: 'all',
          filter: { tools: ['claude'] },
          position: { column: 0, row: 6, width: 6, height: 5 },
        },
      ],
      hidden_projects: ['scratch'],
      project_names: { '/home/me/billing': 'Billing' },
    },
  };

  it('reads a table grouped by the groups the setup found', () => {
    const grouped = {
      ...document,
      payload: {
        ...document.payload,
        widgets: [{ ...document.payload.widgets[0], group_by: 'collection' }],
      },
    };
    const reply = parseDocumentReply(JSON.stringify({ document: grouped }));
    expect(reply).toMatchObject({ ok: true, document: { widgets: [{ groupBy: 'collection' }] } });
  });

  it('keeps what the page draws of the document', () => {
    expect(parseDocumentReply(JSON.stringify({ document }))).toEqual({
      ok: true,
      problem: null,
      document: {
        title: 'Activity',
        period: 'this_week',
        sessions: 'own',
        source: 'ai_sessions',
        hiddenProjects: ['scratch'],
        projectNames: { '/home/me/billing': 'Billing' },
        widgets: [
          {
            id: 'by-project',
            kind: 'table',
            groupBy: 'project',
            period: 'last_week',
            sessions: 'all',
            filter: { tools: ['claude'] },
            position: { column: 0, row: 6, width: 6, height: 5 },
          },
        ],
      },
    });
  });

  it('gives the problem of the file with the last good document', () => {
    const problem = { kind: 'pointer', place: '/payload/widgets/0', message: 'is not a widget' };
    const result = parseDocumentReply(JSON.stringify({ document, error: problem }));
    expect(result.ok && result.problem).toEqual(problem);
    expect(result.ok && result.document?.title).toBe('Activity');
  });

  it('gives a problem with no document when the file has never passed', () => {
    const problem = {
      kind: 'line',
      place: 'line 1, column 2',
      message: 'the file is not valid JSON',
    };
    expect(parseDocumentReply(JSON.stringify({ document: null, error: problem }))).toEqual({
      ok: true,
      document: null,
      problem,
    });
  });

  it('gives the text of an error of the agent', () => {
    expect(parseDocumentReply('{"error":"the dashboard is not available"}')).toEqual({
      ok: false,
      error: 'the dashboard is not available',
    });
  });

  it('takes a null where the agent allows one, as a name left out', () => {
    const withNulls = {
      ...document,
      payload: {
        ...document.payload,
        widgets: [
          {
            id: 'total',
            kind: 'total',
            group_by: null,
            period: null,
            sessions: null,
            filter: { projects: null, tickets: ['a'] },
            position: { column: 0, row: 0, width: 12, height: 2 },
          },
        ],
      },
    };
    const result = parseDocumentReply(JSON.stringify({ document: withNulls }));
    expect(result.ok && result.document?.widgets).toEqual([
      {
        id: 'total',
        kind: 'total',
        filter: { tickets: ['a'] },
        position: { column: 0, row: 0, width: 12, height: 2 },
      },
    ]);
  });

  const place = { column: 0, row: 0, width: 1, height: 1 };
  const withWidget = (widget: Record<string, unknown>): string =>
    JSON.stringify({
      document: {
        title: 't',
        payload: {
          period: 'today',
          sessions: 'own',
          widgets: [{ id: 'a', kind: 'total', position: place, ...widget }],
          hidden_projects: [],
          project_names: {},
        },
      },
    });

  it.each([
    ['no document', '{}', 'no document'],
    [
      'a period the app does not know',
      JSON.stringify({
        document: { title: 't', payload: { period: 'always', sessions: 'own', widgets: [] } },
      }),
      'a period',
    ],
    ['a kind of widget it does not know', withWidget({ kind: 'pie' }), 'a kind of widget'],
    ['a grouping it does not know', withWidget({ group_by: 'colour' }), 'a grouping'],
    ['a choice of sessions it does not know', withWidget({ sessions: 'nobody' }), 'a choice'],
    [
      'a position that is not whole',
      withWidget({ position: { ...place, width: 1.5 } }),
      'whole number',
    ],
    ['a filter that is not lists of names', withWidget({ filter: { tools: [1] } }), 'a filter'],
    [
      'a problem of a kind it does not know',
      JSON.stringify({ document: null, error: { kind: 'other', place: '', message: 'm' } }),
      'a kind of problem',
    ],
    [
      'a document with no widgets list',
      JSON.stringify({ document: { title: 't', payload: { period: 'today', sessions: 'own' } } }),
      'no widgets',
    ],
    [
      'hidden projects that are not a list of names',
      JSON.stringify({
        document: {
          title: 't',
          payload: { period: 'today', sessions: 'own', widgets: [], hidden_projects: [1] },
        },
      }),
      'hidden projects',
    ],
    [
      'project names that are not names',
      JSON.stringify({
        document: {
          title: 't',
          payload: {
            period: 'today',
            sessions: 'own',
            widgets: [],
            hidden_projects: [],
            project_names: { a: 1 },
          },
        },
      }),
      'project names',
    ],
    [
      'a title that is not text',
      JSON.stringify({ document: { title: 7, payload: { widgets: [] } } }),
      'a title',
    ],
  ])('refuses %s', (_name, reply, message) => {
    expect(() => parseDocumentReply(reply)).toThrow(message);
  });
});

describe('parseProjectChange', () => {
  it('takes a project to hide or show, and a project with the name to show for a rename', () => {
    expect(parseProjectChange({ action: 'hide', project: 'billing' })).toEqual({
      action: 'hide',
      project: 'billing',
    });
    expect(parseProjectChange({ action: 'show', project: 'billing' })).toEqual({
      action: 'show',
      project: 'billing',
    });
    expect(
      parseProjectChange({ action: 'rename', project: 'billing', name: ' Invoices ' }),
    ).toEqual({ action: 'rename', project: 'billing', name: 'Invoices' });
  });

  it.each([
    ['not an object', 'hide'],
    ['an action it does not know', { action: 'delete', project: 'a' }],
    ['an empty project', { action: 'hide', project: '  ' }],
    ['a project that is not text', { action: 'hide', project: 7 }],
    ['a name for a hide', { action: 'hide', project: 'a', name: 'b' }],
    ['no name for a rename', { action: 'rename', project: 'a' }],
    ['an empty name', { action: 'rename', project: 'a', name: ' ' }],
    ['a member of its own', { action: 'hide', project: 'a', extra: 1 }],
    ['a project that is too long', { action: 'hide', project: 'a'.repeat(513) }],
    ['a name that is too long', { action: 'rename', project: 'a', name: 'b'.repeat(513) }],
  ])('refuses %s', (_name, value) => {
    expect(parseProjectChange(value)).toBeNull();
  });

  it('writes the line of the socket command', () => {
    expect(projectChangeLine({ action: 'hide', project: 'a b' })).toBe(
      'dashboard-hide {"project":"a b"}',
    );
    expect(projectChangeLine({ action: 'show', project: 'a' })).toBe(
      'dashboard-show {"project":"a"}',
    );
    expect(projectChangeLine({ action: 'rename', project: 'a', name: 'B' })).toBe(
      'dashboard-rename {"project":"a","name":"B"}',
    );
  });
});

const page = { group: 'billing', offset: 0, limit: 20 };

describe('parseSessionsInput', () => {
  it('takes a widget with a group and a page, and nothing else', () => {
    expect(parseSessionsInput({ days: [day], widget: 'by-project', ...page, extra: 1 })).toEqual({
      days: [day],
      widget: 'by-project',
      ...page,
    });
  });

  it('takes the sessions and the grouping of a chart', () => {
    const input = { days: [day], sessions: 'all', groupBy: 'day', filter: { tools: ['codex'] } };
    expect(parseSessionsInput({ ...input, ...page, group: '2026-11-16' })).toEqual({
      ...input,
      ...page,
      group: '2026-11-16',
    });
  });

  it('refuses a widget named with the counting, and a page out of bounds', () => {
    const widget = { days: [day], widget: 'by-project', ...page };
    expect(parseSessionsInput({ ...widget, sessions: 'own' })).toBeNull();
    expect(parseSessionsInput({ ...widget, groupBy: 'day' })).toBeNull();
    expect(parseSessionsInput({ ...widget, limit: 0 })).toBeNull();
    expect(parseSessionsInput({ ...widget, limit: 201 })).toBeNull();
    expect(parseSessionsInput({ ...widget, offset: -1 })).toBeNull();
    expect(parseSessionsInput({ ...widget, group: '' })).toBeNull();
    expect(parseSessionsInput({ days: [day], sessions: 'own', ...page })).toBeNull();
    expect(parseSessionsInput('billing')).toBeNull();
  });
});

describe('the time on screen in a drill-down call', () => {
  const day = { label: '2026-11-16', start: '2026-11-16T00:00Z', end: '2026-11-17T00:00Z' };
  const page = { days: [day], group: 'Safari', offset: 0, limit: 20 };

  it('names the source and no sessions, and is sent in the agent names', () => {
    const input = parseSessionsInput({ ...page, source: 'on_screen', groupBy: 'day' });
    expect(input).toEqual({ ...page, source: 'on_screen', groupBy: 'day' });
    expect(input && dashboardSessionsLine(input)).toBe(
      `dashboard-sessions ${JSON.stringify({ ...page, source: 'on_screen', group_by: 'day' })}`,
    );
  });

  it('refuses sessions for the time on screen, and a source that is not known', () => {
    expect(
      parseSessionsInput({ ...page, source: 'on_screen', sessions: 'own', groupBy: 'day' }),
    ).toBeNull();
    expect(
      parseSessionsInput({ ...page, source: 'elsewhere', sessions: 'own', groupBy: 'day' }),
    ).toBeNull();
  });

  it('keeps the grouping it names, whichever of the five it is', () => {
    for (const groupBy of ['app', 'site', 'project', 'ticket', 'day'] as const) {
      expect(parseSessionsInput({ ...page, source: 'on_screen', groupBy })).toEqual({
        ...page,
        source: 'on_screen',
        groupBy,
      });
    }
  });

  it('sends the filter it names, and only that', () => {
    const filter = { projects: ['billing'], tickets: ['alpha-7'] };
    const input = parseSessionsInput({ ...page, source: 'on_screen', groupBy: 'app', filter });
    expect(input).toEqual({ ...page, source: 'on_screen', groupBy: 'app', filter });
    expect(
      input && JSON.parse(dashboardSessionsLine(input).slice('dashboard-sessions '.length)),
    ).toEqual({
      ...page,
      source: 'on_screen',
      filter,
      group_by: 'app',
    });
    expect(
      parseSessionsInput({ ...page, source: 'on_screen', groupBy: 'app', filter: { projects: 3 } }),
    ).toBeNull();
  });
});

describe('the source of a document', () => {
  const payload = { period: 'this_week', sessions: 'own', hidden_projects: [], project_names: {} };
  const widget = (more: object): object => ({
    id: 'a',
    kind: 'table',
    group_by: 'app',
    position: { column: 0, row: 0, width: 4, height: 4 },
    ...more,
  });

  it('reads the source of the page and of a widget, and the groupings of the time on screen', () => {
    const reply = parseDocumentReply(
      JSON.stringify({
        document: {
          title: 'Activity',
          payload: {
            ...payload,
            source: 'on_screen',
            widgets: [widget({ source: 'ai_sessions' })],
          },
        },
      }),
    );
    expect(reply).toMatchObject({
      ok: true,
      document: { source: 'on_screen', widgets: [{ source: 'ai_sessions', groupBy: 'app' }] },
    });
  });

  it('refuses a source the app does not know', () => {
    const document = { title: 'Activity', payload: { ...payload, source: 'cloud', widgets: [] } };
    expect(() => parseDocumentReply(JSON.stringify({ document }))).toThrow('a source');
  });
});

describe('dashboardSessionsLine', () => {
  it('names the fields as the agent does', () => {
    expect(dashboardSessionsLine({ days: [day], widget: 'by-project', ...page })).toBe(
      `dashboard-sessions ${JSON.stringify({
        days: [day],
        group: 'billing',
        offset: 0,
        limit: 20,
        widget: 'by-project',
      })}`,
    );
    expect(
      JSON.parse(
        dashboardSessionsLine({ days: [day], sessions: 'own', groupBy: 'day', ...page }).slice(
          'dashboard-sessions '.length,
        ),
      ),
    ).toMatchObject({ sessions: 'own', group_by: 'day' });
  });
});

describe('parseSessionsReply', () => {
  const row = {
    start: '2026-11-17T10:00:00+01:00',
    end: '2026-11-17T10:20:00+01:00',
    minutes: 20,
    tool: 'claude-code',
    project: 'billing',
    ticket: 'No ticket',
    session: 'abcd1234',
  };

  it('turns a page into rows with the place of the next one', () => {
    const reply = JSON.stringify({
      group: 'billing',
      total: 3,
      offset: 0,
      limit: 2,
      next_offset: 2,
      sessions: [row, row],
    });
    expect(parseSessionsReply(reply)).toEqual({
      ok: true,
      total: 3,
      nextOffset: 2,
      kind: 'sessions',
      rows: [row, row],
    });
  });

  it('turns a page of the time on screen into blocks, with no title and no address', () => {
    const block = {
      start: '2026-11-17T10:00:00+00:00',
      end: '2026-11-17T10:05:20+00:00',
      seconds: 320,
      app: 'Safari',
      site: 'example.org',
      project: 'billing',
      ticket: 'No ticket',
    };
    const reply = JSON.stringify({
      source: 'on_screen',
      group: 'Safari',
      total: 1,
      offset: 0,
      limit: 20,
      next_offset: null,
      blocks: [block],
    });
    expect(parseSessionsReply(reply)).toEqual({
      ok: true,
      total: 1,
      nextOffset: null,
      kind: 'blocks',
      rows: [block],
    });
  });

  it('refuses a block that is not times with an app and a site', () => {
    const reply = JSON.stringify({ total: 1, next_offset: null, blocks: [{ seconds: 1 }] });
    expect(() => parseSessionsReply(reply)).toThrow('not times with an app and a site');
  });

  it('has no next page after the last one', () => {
    const reply = JSON.stringify({ total: 1, offset: 0, next_offset: null, sessions: [row] });
    expect(parseSessionsReply(reply)).toMatchObject({ ok: true, nextOffset: null });
  });

  it('gives the error of the agent, and says when it is still reading', () => {
    expect(parseSessionsReply('{"error":"no"}')).toEqual({ ok: false, error: 'no' });
    expect(parseSessionsReply(`{"error":"${readingHistoryError}"}`)).toEqual({
      ok: false,
      error: readingHistoryError,
      reading: true,
    });
  });

  it('refuses a session that is not times with a tool', () => {
    const reply = JSON.stringify({ total: 1, offset: 0, next_offset: null, sessions: [{}] });
    expect(() => parseSessionsReply(reply)).toThrow('not times with a tool');
  });
});

describe('the summary of a day', () => {
  it('takes a day and a widget, and nothing else', () => {
    expect(parseSummaryInput({ day, widget: 'end-of-day' })).toEqual({ day, widget: 'end-of-day' });
    expect(parseSummaryInput({ day, widget: 'end-of-day', sessions: 'own' })).toBeNull();
    expect(parseSummaryInput({ day: [day], widget: 'end-of-day' })).toBeNull();
    expect(parseSummaryInput({ day, widget: 'End Of Day' })).toBeNull();
  });

  it('asks for the one day', () => {
    expect(dashboardSummaryLine({ day, widget: 'end-of-day' })).toBe(
      `dashboard-summary ${JSON.stringify({ day, widget: 'end-of-day' })}`,
    );
  });

  it('turns the reply into lines', () => {
    const reply =
      '{"day":"2026-11-16","total_minutes":80,"lines":["A-1: 1 h","No ticket (b): 20 min"]}';
    expect(parseSummaryReply(reply)).toEqual({
      ok: true,
      lines: ['A-1: 1 h', 'No ticket (b): 20 min'],
    });
    expect(parseSummaryReply('{"error":"no"}')).toEqual({ ok: false, error: 'no' });
    expect(() => parseSummaryReply('{"day":"d","total_minutes":1,"lines":[1]}')).toThrow();
  });
});
