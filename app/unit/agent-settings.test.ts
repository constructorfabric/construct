import { describe, expect, it } from 'vitest';
import {
  collectionSetLine,
  parseCollectionInput,
  parseModelInput,
  parseSettingsReply,
  parseTestReply,
  removeKeyLine,
  withCollectionReply,
  settingsSetLine,
  settingsTestLine,
} from '../src/agent-settings';

describe('the command lines', () => {
  it('sends the key only when one was typed', () => {
    expect(settingsSetLine({ baseUrl: 'https://x/v1', model: 'm' })).toBe(
      'settings-set {"model":{"base_url":"https://x/v1","model":"m"}}',
    );
    expect(settingsSetLine({ baseUrl: 'https://x/v1', model: 'm', apiKey: '' })).not.toContain(
      'api_key',
    );
    expect(settingsSetLine({ baseUrl: 'https://x/v1', model: 'm', apiKey: 'sk-1' })).toBe(
      'settings-set {"model":{"base_url":"https://x/v1","model":"m","api_key":"sk-1"}}',
    );
  });

  it('stays on one line, whatever the key holds', () => {
    const line = settingsSetLine({ baseUrl: 'https://x', model: 'm', apiKey: 'a\nb' });
    expect(line.split('\n')).toHaveLength(1);
  });

  it('removes the key with a flag', () => {
    expect(removeKeyLine).toBe('settings-set {"model":{"remove_key":true}}');
  });

  it('leaves out of a test what was not given', () => {
    expect(settingsTestLine({ baseUrl: '', model: '' })).toBe('settings-test {}');
    expect(settingsTestLine({ baseUrl: 'http://127.0.0.1:1', model: 'm', apiKey: 'k' })).toBe(
      'settings-test {"base_url":"http://127.0.0.1:1","model":"m","api_key":"k"}',
    );
  });
});

describe('parseModelInput', () => {
  it('accepts the form values, with or without a key', () => {
    expect(parseModelInput({ baseUrl: 'a', model: 'b' })).toEqual({ baseUrl: 'a', model: 'b' });
    expect(parseModelInput({ baseUrl: 'a', model: 'b', apiKey: 'c' })).toEqual({
      baseUrl: 'a',
      model: 'b',
      apiKey: 'c',
    });
  });

  it.each([
    ['null', null],
    ['a string', 'x'],
    ['no address', { model: 'b' }],
    ['a model that is not text', { baseUrl: 'a', model: 1 }],
    ['a key that is not text', { baseUrl: 'a', model: 'b', apiKey: 1 }],
  ])('refuses %s', (_name, value) => {
    expect(parseModelInput(value)).toBeNull();
  });

  it('drops fields it does not know', () => {
    expect(parseModelInput({ baseUrl: 'a', model: 'b', extra: 1 })).toEqual({
      baseUrl: 'a',
      model: 'b',
    });
  });
});

describe('parseSettingsReply', () => {
  it('reads the model section', () => {
    const line =
      '{"model":{"base_url":"u","model":"m","api_key_set":true},' +
      '"collection":{"enabled":false,"claude_code":true,"codex":false},"notice":"unreadable"}';
    expect(parseSettingsReply(line)).toEqual({
      ok: true,
      model: { baseUrl: 'u', model: 'm', apiKeySet: true },
      collection: { enabled: false, claudeCode: true, codex: false },
      notice: 'unreadable',
    });
  });

  it.each([
    ['sends none', ''],
    ['sends null', ',"notice":null'],
    ['sends something that is not a text', ',"notice":5'],
  ])('has no notice when the agent %s', (_name, notice) => {
    const line =
      '{"model":{"base_url":"u","model":"m","api_key_set":true},' +
      `"collection":{"enabled":true,"claude_code":true,"codex":true}${notice}}`;
    expect(parseSettingsReply(line)).toEqual({
      ok: true,
      model: { baseUrl: 'u', model: 'm', apiKeySet: true },
      collection: { enabled: true, claudeCode: true, codex: true },
      notice: null,
    });
  });

  it('turns an agent error into a result', () => {
    expect(parseSettingsReply('{"error":"invalid api_key"}')).toEqual({
      ok: false,
      error: 'invalid api_key',
    });
  });

  it.each([
    ['not JSON', 'x', /JSON/],
    ['not an object', '1', /not a JSON object/],
    ['no model', '{}', /no model settings/],
    [
      'a key flag that is not a boolean',
      '{"model":{"base_url":"","model":"","api_key_set":1}}',
      /no model/,
    ],
    [
      'no collection',
      '{"model":{"base_url":"","model":"","api_key_set":true}}',
      /no collection settings/,
    ],
    [
      'a switch that is not a boolean',
      '{"model":{"base_url":"","model":"","api_key_set":true},' +
        '"collection":{"enabled":true,"claude_code":"on","codex":true}}',
      /no collection settings/,
    ],
  ])('rejects %s', (_name, line, message) => {
    expect(() => parseSettingsReply(line)).toThrow(message);
  });
});

describe('parseCollectionInput', () => {
  it('drops names it does not know beside the ones it does', () => {
    expect(parseCollectionInput({ codex: true, model: {} })).toEqual({ codex: true });
  });

  it('keeps the switches given', () => {
    expect(parseCollectionInput({ codex: false })).toEqual({ codex: false });
    expect(parseCollectionInput({ enabled: true, claudeCode: false, codex: true })).toEqual({
      enabled: true,
      claudeCode: false,
      codex: true,
    });
  });

  it.each([
    ['nothing', {}],
    ['not an object', 1],
    ['a switch that is not a boolean', { codex: 'on' }],
    ['only names it does not know', { claude_code: false, model: {} }],
    ['a valid switch beside an invalid one', { enabled: true, codex: 'yes' }],
  ])('refuses %s', (_name, value) => {
    expect(parseCollectionInput(value)).toBeNull();
  });
});

describe('collectionSetLine', () => {
  it('uses the names of the agent and leaves out what is not given', () => {
    expect(collectionSetLine({ claudeCode: false })).toBe(
      'settings-set {"collection":{"claude_code":false}}',
    );
    expect(collectionSetLine({ enabled: true, claudeCode: true, codex: false })).toBe(
      'settings-set {"collection":{"enabled":true,"claude_code":true,"codex":false}}',
    );
  });
});

describe('withCollectionReply', () => {
  const current = {
    ok: true,
    model: { baseUrl: 'new', model: 'new', apiKeySet: false },
    collection: { enabled: true, claudeCode: true, codex: true },
    notice: 'old notice',
  } as const;
  const reply = { collection: { enabled: true, claudeCode: true, codex: false }, notice: null };

  it('takes the collection and the notice and keeps the model the page holds now', () => {
    expect(withCollectionReply(current, reply)).toEqual({
      ok: true,
      model: { baseUrl: 'new', model: 'new', apiKeySet: false },
      ...reply,
    });
  });

  it.each([
    ['nothing loaded', null],
    ['a failed load', { ok: false, error: 'no agent' } as const],
  ])('leaves %s as it is', (_name, state) => {
    expect(withCollectionReply(state, reply)).toEqual(state);
  });
});

describe('parseTestReply', () => {
  it.each([
    ['{"result":"works","text":"pong"}', { result: 'works', text: 'pong' }],
    ['{"result":"key refused"}', { result: 'key refused' }],
    ['{"result":"model not found"}', { result: 'model not found' }],
    ['{"result":"unreachable"}', { result: 'unreachable' }],
    ['{"result":"timeout"}', { result: 'timeout' }],
    ['{"result":"error","status":502}', { result: 'error', status: 502 }],
    ['{"error":"no model address is set"}', { result: 'failed', error: 'no model address is set' }],
  ])('reads %s', (line, expected) => {
    expect(parseTestReply(line)).toEqual(expected);
  });

  it.each([
    ['works without text', '{"result":"works"}'],
    ['an error without a status', '{"result":"error"}'],
    ['a result it does not know', '{"result":"fine"}'],
  ])('rejects %s', (_name, line) => {
    expect(() => parseTestReply(line)).toThrow();
  });
});
