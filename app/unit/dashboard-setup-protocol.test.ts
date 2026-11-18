import { describe, expect, it } from 'vitest';
import { isRunning, parseSetupReply, setupStartLine } from '../src/dashboard-setup-protocol';

const counts = '"sessions":3,"text_bytes":2048,"sent":100,"received":40';

describe('a setup reply', () => {
  it('gives the state and the counts of a run that goes', () => {
    const result = parseSetupReply(`{"state":"reading_and_building",${counts}}`);
    expect(result).toEqual({
      ok: true,
      status: {
        state: 'reading_and_building',
        sessions: 3,
        textBytes: 2048,
        sent: 100,
        received: 40,
      },
    });
  });

  it('gives the report of a run that is done and the reason of one that failed', () => {
    const done = parseSetupReply(`{"state":"done",${counts},"report":"# Found"}`);
    expect(done.ok && done.status.report).toBe('# Found');
    const failed = parseSetupReply(`{"state":"failed",${counts},"error":"no session to read"}`);
    expect(failed.ok && failed.status.error).toBe('no session to read');
  });

  it('gives the error of the agent as it is', () => {
    expect(parseSetupReply('{"error":"no model is set"}')).toEqual({
      ok: false,
      error: 'no model is set',
    });
  });

  it('refuses a state the window does not know and a count that is not a number', () => {
    expect(() => parseSetupReply(`{"state":"paused",${counts}}`)).toThrow();
    expect(() =>
      parseSetupReply('{"state":"done","sessions":"3","text_bytes":0,"sent":0,"received":0}'),
    ).toThrow();
    expect(() => parseSetupReply('[]')).toThrow();
  });

  it('says that only the two steps of a run are running', () => {
    expect(isRunning('writing_files')).toBe(true);
    expect(isRunning('reading_and_building')).toBe(true);
    for (const state of ['idle', 'done', 'failed', 'cancelled'] as const) {
      expect(isRunning(state)).toBe(false);
    }
  });

  it('asks for the 30 days and nothing else', () => {
    expect(setupStartLine).toBe('dashboard-setup-start {"days":30}');
  });
});
