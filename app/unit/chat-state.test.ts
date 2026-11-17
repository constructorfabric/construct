import { describe, expect, it } from 'vitest';
import { chatReducer, initialChatState } from '../src/ui/chat-state';
import type { ChatAction, ChatState } from '../src/ui/chat-state';

function run(actions: readonly ChatAction[], from: ChatState = initialChatState): ChatState {
  return actions.reduce(chatReducer, from);
}

describe('chatReducer', () => {
  it('grows one assistant message from the pieces', () => {
    const state = run([
      { type: 'sending', message: 'hi' },
      { type: 'started', conversation: 'c1' },
      { type: 'delta', text: 'Hel' },
      { type: 'delta', text: 'lo' },
    ]);
    expect(state.messages).toEqual([
      { role: 'user', text: 'hi' },
      { role: 'assistant', text: 'Hello' },
    ]);
    expect(state.running).toBe(true);
    expect(state.active).toBe('c1');
  });

  it('puts a tool line between the messages and starts a new one with the next text', () => {
    const state = run([
      { type: 'sending', message: 'hi' },
      { type: 'started', conversation: 'c1' },
      { type: 'delta', text: 'Let me look.' },
      { type: 'tool', line: 'Read notes.txt' },
      { type: 'tool', line: 'Wrote out.txt' },
      { type: 'delta', text: 'Done' },
      { type: 'delta', text: '.' },
    ]);
    expect(state.messages).toEqual([
      { role: 'user', text: 'hi' },
      { role: 'assistant', text: 'Let me look.' },
      { role: 'tool', text: 'Read notes.txt' },
      { role: 'tool', text: 'Wrote out.txt' },
      { role: 'assistant', text: 'Done.' },
    ]);
  });

  it('keeps a tool line that ends the answer, and ignores one when no answer runs', () => {
    const ended = run([
      { type: 'sending', message: 'hi' },
      { type: 'tool', line: 'Read notes.txt' },
      { type: 'done', stopped: false },
    ]);
    expect(ended.messages.at(-1)).toEqual({ role: 'tool', text: 'Read notes.txt' });
    expect(ended.running).toBe(false);
    expect(chatReducer(ended, { type: 'tool', line: 'late' })).toBe(ended);
  });

  it('keeps the part after Stop and offers Try again', () => {
    const state = run([
      { type: 'sending', message: 'hi' },
      { type: 'started', conversation: 'c1' },
      { type: 'delta', text: 'Hel' },
      { type: 'done', stopped: true },
    ]);
    expect(state.running).toBe(false);
    expect(state.failure).toEqual({ kind: 'stopped' });
    expect(state.messages.at(-1)).toEqual({ role: 'assistant', text: 'Hel' });
    expect(state.lastMessage).toBe('hi');
  });

  it('does not repeat a message the agent refused before storing it', () => {
    const refused = run([
      { type: 'sending', message: 'hi' },
      { type: 'failed', reason: 'no model' },
    ]);
    const again = run([{ type: 'sending', message: 'hi' }], refused);
    expect(again.messages).toEqual([{ role: 'user', text: 'hi' }]);
    expect(again.failure).toBeNull();
  });

  it('repeats a message the agent stored, as the agent will have it twice', () => {
    const failed = run([
      { type: 'sending', message: 'hi' },
      { type: 'started', conversation: 'c1' },
      { type: 'failed', reason: 'key refused' },
    ]);
    const again = run([{ type: 'sending', message: 'hi' }], failed);
    expect(again.messages).toEqual([
      { role: 'user', text: 'hi' },
      { role: 'user', text: 'hi' },
    ]);
  });

  it('ignores a piece that arrives when no answer runs', () => {
    expect(run([{ type: 'delta', text: 'late' }])).toEqual(initialChatState);
  });

  it('ignores a conversation that came late, after the user went elsewhere', () => {
    const state = run([
      { type: 'opening', conversation: 'a' },
      { type: 'new-chat' },
      { type: 'opened', conversation: 'a', messages: [{ role: 'user', text: 'old' }] },
    ]);
    expect(state.messages).toEqual([]);
    expect(state.active).toBe('');
  });
});
