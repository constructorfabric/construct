import { describe, expect, it } from 'vitest';
import { parsePart, parseStatusReply } from '../src/agent-status';

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
