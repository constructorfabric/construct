import { describe, expect, it } from 'vitest';
import { parseStatusReply } from '../src/agent-status';

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
