import { describe, expect, it } from 'vitest';
import { parsePart, parseStatusReply, summarizeCollection } from '../src/agent-status';
import type { Connector } from '../src/agent-status';

describe('parseStatusReply', () => {
  it('turns a running reply into a running status and drops unknown fields', () => {
    expect(parseStatusReply('{"status":"running","version":"0.1.0","pid":42,"extra":"x"}')).toEqual(
      { state: 'running', version: '0.1.0', pid: 42, startAtLogin: 'unknown' },
    );
  });

  it('carries the model name the agent reports', () => {
    expect(
      parseStatusReply('{"status":"running","version":"0.1.0","pid":42,"model":"example-model"}'),
    ).toEqual({
      state: 'running',
      version: '0.1.0',
      pid: 42,
      model: 'example-model',
      startAtLogin: 'unknown',
    });
  });

  it.each(['registered', 'not_registered'] as const)('carries the service field %s', (service) => {
    expect(
      parseStatusReply(`{"status":"running","version":"0.1.0","pid":42,"service":"${service}"}`),
    ).toEqual({ state: 'running', version: '0.1.0', pid: 42, startAtLogin: service });
  });

  it('reads a reply of an older agent, with no service field, as unknown', () => {
    expect(parseStatusReply('{"status":"running","version":"0.1.0","pid":42}')).toEqual({
      state: 'running',
      version: '0.1.0',
      pid: 42,
      startAtLogin: 'unknown',
    });
  });

  it('carries the state and the reason of the database and the Construct gear', () => {
    expect(
      parseStatusReply(
        JSON.stringify({
          status: 'running',
          version: '0.1.0',
          pid: 42,
          database: { state: 'failed', reason: 'pgvector is not built for this platform yet' },
          construct: { state: 'failed', reason: 'the database is not running' },
        }),
      ),
    ).toEqual({
      state: 'running',
      version: '0.1.0',
      pid: 42,
      startAtLogin: 'unknown',
      database: { state: 'failed', reason: 'pgvector is not built for this platform yet' },
      construct: { state: 'failed', reason: 'the database is not running' },
    });
  });

  it('reads the plain state strings of an older agent', () => {
    expect(
      parseStatusReply(
        '{"status":"running","version":"0.1.0","pid":42,"database":"failed","construct":"starting"}',
      ),
    ).toEqual({
      state: 'running',
      version: '0.1.0',
      pid: 42,
      startAtLogin: 'unknown',
      database: { state: 'failed' },
      construct: { state: 'starting' },
    });
  });

  it('skips a part it does not know and still reads the rest of the reply', () => {
    expect(
      parseStatusReply(
        JSON.stringify({
          status: 'running',
          version: '0.1.0',
          pid: 42,
          model: 'm',
          database: { state: 'sleeping', reason: 'x' },
          construct: 7,
        }),
      ),
    ).toEqual({ state: 'running', version: '0.1.0', pid: 42, model: 'm', startAtLogin: 'unknown' });
  });

  it.each([
    ['ready', { state: 'ready' }],
    ['starting', { state: 'starting' }],
    [{ state: 'ready' }, { state: 'ready' }],
    // A ready part has no reason to show, whatever the agent sends.
    [{ state: 'ready', reason: 'old text' }, { state: 'ready' }],
    [
      { state: 'starting', reason: 'waiting for the database' },
      { state: 'starting', reason: 'waiting for the database' },
    ],
    [
      { state: 'failed', reason: '  PostgreSQL does not start ' },
      { state: 'failed', reason: 'PostgreSQL does not start' },
    ],
    [{ state: 'failed', reason: '' }, { state: 'failed' }],
    [{ state: 'failed', reason: 3 }, { state: 'failed' }],
    [{ state: 'failed' }, { state: 'failed' }],
  ])('reads the part %j', (value, part) => {
    expect(parsePart(value)).toEqual(part);
  });

  it.each([['unknown'], [''], [null], [7], [[]], [{}], [{ state: 'Ready' }], [{ state: 1 }]])(
    'skips the part %j',
    (value) => {
      expect(parsePart(value)).toBeUndefined();
    },
  );

  it('reads the grants of the screen row and skips what it does not know', () => {
    const line = JSON.stringify({
      status: 'running',
      version: '0.1.0',
      pid: 1,
      screen: {
        state: 'needs_permission',
        grants: [
          { name: 'accessibility', state: 'granted' },
          { name: 'automation', state: 'browser_not_running' },
          { name: 'screen_recording', state: 'not_asked' },
          { name: 'something_new', state: 'granted' },
          { name: 'input_monitoring', state: 'maybe' },
          'junk',
        ],
      },
    });
    expect(parseStatusReply(line)).toMatchObject({
      grants: [
        { name: 'accessibility', state: 'granted' },
        { name: 'automation', state: 'browser_not_running' },
        { name: 'screen_recording', state: 'not_asked' },
      ],
    });
  });

  it.each([
    ['no screen row', ''],
    ['a screen row with no grants', ',"screen":{"state":"off"}'],
    ['a screen row that is not an object', ',"screen":3'],
  ])('has no grants with %s', (_name, extra) => {
    const line = `{"status":"running","version":"0.1.0","pid":1${extra}}`;
    expect(parseStatusReply(line)).not.toHaveProperty('grants');
  });

  it.each([
    ['not JSON', 'running', /JSON/],
    ['not an object', '"running"', /not a JSON object/],
    ['null', 'null', /not a JSON object/],
    [
      'a status that is not running',
      '{"status":"stopping","version":"0.1.0","pid":1}',
      /did not report running/,
    ],
    ['no version', '{"status":"running","pid":1}', /no version/],
    ['a version that is not text', '{"status":"running","version":1,"pid":1}', /no version/],
    [
      'a model that is not text',
      '{"status":"running","version":"0.1.0","pid":1,"model":3}',
      /model/,
    ],
    [
      'a service that is neither value',
      '{"status":"running","version":"0.1.0","pid":1,"service":"maybe"}',
      /service/,
    ],
    ['no pid', '{"status":"running","version":"0.1.0"}', /no pid/],
    ['a pid that is not a number', '{"status":"running","version":"0.1.0","pid":"1"}', /no pid/],
  ])('rejects a reply with %s', (_name, line, message) => {
    expect(() => parseStatusReply(line)).toThrow(message);
  });
});

const base = { status: 'running', version: '0.1.0', pid: 42 };

describe('the connectors of a status reply', () => {
  const at = '2026-01-02T03:04:05Z';

  it('keeps the other fields and reads each tool', () => {
    const status = parseStatusReply(
      JSON.stringify({
        ...base,
        model: 'example-model',
        connectors: {
          'claude-code': { turns: 7, skipped: 1, scanning: false, last_turn: at },
          codex: { turns: 0, skipped: 0, scanning: true },
        },
      }),
    );
    expect(status).toEqual({
      state: 'running',
      version: '0.1.0',
      pid: 42,
      model: 'example-model',
      startAtLogin: 'unknown',
      connectors: [
        {
          name: 'claude-code',
          state: 'collecting',
          turns: 7,
          skipped: 1,
          unreadable: 0,
          lastTurnAt: Date.parse(at),
        },
        { name: 'codex', state: 'scanning', turns: 0, skipped: 0, unreadable: 0 },
      ],
    });
  });

  it('reads an off tool and a failed one with its reason, a failure outranking the rest', () => {
    const status = parseStatusReply(
      JSON.stringify({
        ...base,
        connectors: {
          'claude-code': {
            turns: 3,
            skipped: 0,
            scanning: true,
            off: true,
            unreadable: 2,
            error: ' the session files cannot be listed ',
          },
          codex: { turns: 0, skipped: 0, scanning: false, off: true },
        },
      }),
    );
    expect(status).toMatchObject({
      connectors: [
        {
          name: 'claude-code',
          state: 'failed',
          reason: 'the session files cannot be listed',
          unreadable: 2,
        },
        { name: 'codex', state: 'off' },
      ],
    });
  });

  it('reads an error that is not a text, or is empty, as a failed tool with no reason', () => {
    const status = parseStatusReply(
      JSON.stringify({
        ...base,
        connectors: {
          a: { turns: 1, error: true },
          b: { turns: 1, error: '  ' },
          c: { turns: 1, error: null },
          d: [],
        },
      }),
    );
    expect(status).toMatchObject({
      connectors: [
        { name: 'a', state: 'failed' },
        { name: 'b', state: 'failed' },
        { name: 'c', state: 'collecting' },
      ],
    });
    expect(status).not.toHaveProperty('connectors.0.reason');
    expect(status).not.toHaveProperty('connectors.1.reason');
    expect(status).not.toHaveProperty('connectors.3');
  });

  it('parses a reply with no connectors, or with a shape or a field it does not know', () => {
    expect(parseStatusReply(JSON.stringify(base))).not.toHaveProperty('connectors');
    expect(parseStatusReply(JSON.stringify({ ...base, connectors: [1] }))).not.toHaveProperty(
      'connectors',
    );
    const status = parseStatusReply(
      JSON.stringify({
        ...base,
        connectors: {
          a: 'broken',
          b: { turns: 'many', last_turn: 'yesterday', future: true, scanning: false },
        },
      }),
    );
    expect(status).toMatchObject({
      connectors: [{ name: 'b', state: 'collecting', turns: 0, skipped: 0 }],
    });
    expect(status).not.toHaveProperty('connectors.0.lastTurnAt');
  });
});

describe('summarizeCollection', () => {
  const now = Date.parse('2026-01-02T03:04:05Z');
  const tool = (fields: Partial<Connector>): Connector => ({
    name: 'codex',
    state: 'collecting',
    turns: 1,
    skipped: 0,
    unreadable: 0,
    ...fields,
  });

  it('says nothing for an agent with no connectors', () => {
    expect(summarizeCollection(undefined, now)).toBeUndefined();
    expect(summarizeCollection([], now)).toBeUndefined();
  });

  it('says collecting for a turn stored within a minute, and nothing after that', () => {
    expect(summarizeCollection([tool({ lastTurnAt: now - 5_000 })], now)).toBe('collecting');
    expect(summarizeCollection([tool({ lastTurnAt: now - 60_001 })], now)).toBeUndefined();
    expect(summarizeCollection([tool({})], now)).toBeUndefined();
    // A time after the app's clock is not a turn just stored.
    expect(summarizeCollection([tool({ lastTurnAt: now + 5_000 })], now)).toBeUndefined();
  });

  it('does not count a turn of a tool that is off or scanning as data just collected', () => {
    const recent = now - 5_000;
    expect(summarizeCollection([tool({ state: 'off', lastTurnAt: recent })], now)).toBe('off');
    expect(summarizeCollection([tool({ state: 'scanning', lastTurnAt: recent })], now)).toBe(
      'scanning',
    );
  });

  it('says off only when every tool is off', () => {
    expect(summarizeCollection([tool({ state: 'off' }), tool({ state: 'off' })], now)).toBe('off');
    expect(summarizeCollection([tool({ state: 'off' }), tool({})], now)).toBeUndefined();
  });

  it('says scanning for a tool reading its files, and failed above all', () => {
    expect(summarizeCollection([tool({ state: 'scanning' })], now)).toBe('scanning');
    expect(
      summarizeCollection(
        [tool({ lastTurnAt: now - 1_000 }), tool({ state: 'failed', reason: 'x' })],
        now,
      ),
    ).toBe('failed');
  });
});
