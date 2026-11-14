import { describe, expect, it } from 'vitest';
import { parseStatusReply } from '../src/agent-status';

describe('parseStatusReply', () => {
  it('turns a running reply into a running status and drops unknown fields', () => {
    expect(parseStatusReply('{"status":"running","version":"0.1.0","pid":42,"extra":"x"}')).toEqual(
      { state: 'running', version: '0.1.0', pid: 42 },
    );
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
    ['no pid', '{"status":"running","version":"0.1.0"}', /no pid/],
    ['a pid that is not a number', '{"status":"running","version":"0.1.0","pid":"1"}', /no pid/],
  ])('rejects a reply with %s', (_name, line, message) => {
    expect(() => parseStatusReply(line)).toThrow(message);
  });
});
