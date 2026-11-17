import { describe, expect, it } from 'vitest';
import {
  chatSendLine,
  isConversationId,
  parseChatEvent,
  parseChatInput,
  parseDeleteReply,
  parseGetLines,
  parseListReply,
} from '../src/chat-protocol';

const id = '3f2a9c1e-0b7d-4c55-9a10-6d2b8e4f7a01';

describe('parseChatEvent', () => {
  it('reads the four events', () => {
    expect(parseChatEvent(`{"event":"start","conversation":"${id}"}`)).toEqual({
      event: 'start',
      conversation: id,
    });
    expect(parseChatEvent('{"event":"delta","text":"Hel"}')).toEqual({
      event: 'delta',
      text: 'Hel',
    });
    expect(parseChatEvent('{"event":"done"}')).toEqual({ event: 'done', stopped: false });
    expect(parseChatEvent('{"event":"done","stopped":true}')).toEqual({
      event: 'done',
      stopped: true,
    });
    expect(parseChatEvent('{"event":"error","reason":"key refused"}')).toEqual({
      event: 'error',
      reason: 'key refused',
    });
  });

  it('reads the line of a tool call', () => {
    expect(parseChatEvent('{"event":"tool","line":"Wrote notes.txt"}')).toEqual({
      event: 'tool',
      line: 'Wrote notes.txt',
    });
    expect(() => parseChatEvent('{"event":"tool"}')).toThrow();
    expect(() => parseChatEvent('{"event":"tool","line":3}')).toThrow();
  });

  it('refuses what is not an event', () => {
    expect(() => parseChatEvent('{"event":"delta"}')).toThrow();
    expect(() => parseChatEvent('{"event":5}')).toThrow();
    expect(() => parseChatEvent('{}')).toThrow();
    expect(() => parseChatEvent('[]')).toThrow();
  });

  it('skips an event it does not know', () => {
    expect(parseChatEvent('{"event":"other"}')).toBeNull();
  });
});

describe('the lines the app sends', () => {
  it('keeps a message with a newline inside one line', () => {
    const line = chatSendLine({ conversation: '', message: 'a\nb "c"' });
    expect(line.split('\n')).toHaveLength(1);
    expect(JSON.parse(line.slice('chat-send '.length))).toEqual({
      conversation: '',
      message: 'a\nb "c"',
    });
  });

  it('accepts only a conversation that is empty or an id', () => {
    expect(parseChatInput({ conversation: '', message: 'hi' })).toEqual({
      conversation: '',
      message: 'hi',
    });
    expect(parseChatInput({ conversation: id, message: 'hi' })?.conversation).toBe(id);
    expect(parseChatInput({ conversation: 'all', message: 'hi' })).toBeNull();
    expect(parseChatInput({ conversation: '', message: 5 })).toBeNull();
    expect(parseChatInput('x')).toBeNull();
    expect(isConversationId(`${id} extra`)).toBe(false);
    expect(isConversationId(`${id}\nchat-delete all`)).toBe(false);
  });
});

describe('the replies', () => {
  it('lists the conversations in the agent order', () => {
    const line = JSON.stringify({
      conversations: [
        { conversation: 'b', title: 'second', time: 't2' },
        { conversation: 'a', title: 'first', time: 't1' },
      ],
    });
    expect(parseListReply(line)).toEqual({
      ok: true,
      conversations: [
        { id: 'b', title: 'second', time: 't2' },
        { id: 'a', title: 'first', time: 't1' },
      ],
    });
    expect(parseListReply('{"error":"cannot read"}')).toEqual({ ok: false, error: 'cannot read' });
  });

  it('shows the turns of a conversation and skips the tool records', () => {
    const lines = [
      '{"kind":"turn","role":"user","time":"1","text":"hi"}',
      '{"kind":"toolcall","time":"2","call_id":"c","name":"recall_facts","arguments":"{}"}',
      '{"kind":"toolresult","time":"3","call_id":"c","text":"none"}',
      '{"kind":"turn","role":"assistant","time":"4","text":"hello"}',
    ];
    expect(parseGetLines(lines)).toEqual({
      ok: true,
      messages: [
        { role: 'user', text: 'hi' },
        { role: 'assistant', text: 'hello' },
      ],
    });
    expect(parseGetLines(['{"error":"unknown conversation"}'])).toEqual({
      ok: false,
      error: 'unknown conversation',
    });
  });

  it('keeps the lines of tool calls in order with the turns', () => {
    const lines = [
      '{"kind":"turn","role":"user","time":"1","text":"hi"}',
      '{"kind":"toolcall","hidden":true,"time":"2","call_id":"c","name":"Read","arguments":"{}"}',
      '{"kind":"turn","role":"tool","time":"2","text":"Read notes.txt"}',
      '{"kind":"toolresult","hidden":true,"time":"3","call_id":"c","text":"secret"}',
      '{"kind":"turn","role":"assistant","time":"4","text":"hello"}',
    ];
    expect(parseGetLines(lines)).toEqual({
      ok: true,
      messages: [
        { role: 'user', text: 'hi' },
        { role: 'tool', text: 'Read notes.txt' },
        { role: 'assistant', text: 'hello' },
      ],
    });
  });

  it('skips a turn of a role it does not know', () => {
    const lines = [
      '{"kind":"turn","role":"user","time":"1","text":"hi"}',
      '{"kind":"turn","role":"other","time":"3","text":"x"}',
      '{"kind":"turn","role":"assistant","time":"4","text":"hello"}',
    ];
    expect(parseGetLines(lines)).toEqual({
      ok: true,
      messages: [
        { role: 'user', text: 'hi' },
        { role: 'assistant', text: 'hello' },
      ],
    });
    expect(() => parseGetLines(['{"kind":"turn","role":5,"text":"x"}'])).toThrow();
  });

  it('reads a delete reply', () => {
    expect(parseDeleteReply('{"deleted":2}')).toEqual({ ok: true });
    expect(parseDeleteReply('{"error":"busy"}')).toEqual({ ok: false, error: 'busy' });
  });
});
