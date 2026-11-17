import type { ChatMessage, ConversationSummary } from '../chat-protocol';

/** Why the last answer did not come out whole: the agent's reason, or the user's Stop. */
export type Failure =
  | { readonly kind: 'reason'; readonly reason: string }
  | { readonly kind: 'stopped' };

export interface ChatState {
  readonly conversations: readonly ConversationSummary[];
  /** A text for the sidebar when the list or a delete did not work. */
  readonly notice: string | null;
  /** The open conversation; empty for a new chat. */
  readonly active: string;
  readonly messages: readonly ChatMessage[];
  /** Whether the open conversation is being read from the agent. */
  readonly loading: boolean;
  readonly loadError: string | null;
  readonly running: boolean;
  /** Whether the assistant's message of the running answer has started. */
  readonly streaming: boolean;
  readonly failure: Failure | null;
  /** What Try again sends. */
  readonly lastMessage: string;
  /** Whether the agent has stored the last message: it said `start` for it. */
  readonly stored: boolean;
}

/** What an empty thread is: nothing shown, nothing to retry. */
const blankThread = {
  messages: [],
  loading: false,
  loadError: null,
  failure: null,
  lastMessage: '',
  stored: false,
} as const;

export const initialChatState: ChatState = {
  conversations: [],
  notice: null,
  active: '',
  messages: [],
  loading: false,
  loadError: null,
  running: false,
  streaming: false,
  failure: null,
  lastMessage: '',
  stored: false,
};

export type ChatAction =
  | { readonly type: 'listed'; readonly conversations: readonly ConversationSummary[] }
  | { readonly type: 'notice'; readonly notice: string | null }
  | { readonly type: 'new-chat' }
  | { readonly type: 'opening'; readonly conversation: string }
  | {
      readonly type: 'opened';
      readonly conversation: string;
      readonly messages: readonly ChatMessage[];
    }
  | { readonly type: 'open-failed'; readonly conversation: string; readonly error: string }
  | { readonly type: 'sending'; readonly message: string }
  | { readonly type: 'send-refused'; readonly reason: string }
  | { readonly type: 'started'; readonly conversation: string }
  | { readonly type: 'delta'; readonly text: string }
  | { readonly type: 'tool'; readonly line: string }
  | { readonly type: 'done'; readonly stopped: boolean }
  | { readonly type: 'failed'; readonly reason: string };

/** The user sends `message`. One the agent refused before it stored it is on screen already. */
function sending(state: ChatState, message: string): ChatState {
  const last = state.messages.at(-1);
  const shown =
    state.failure !== null && !state.stored && last?.role === 'user' && last.text === message;
  return {
    ...state,
    messages: shown ? state.messages : [...state.messages, { role: 'user', text: message }],
    running: true,
    streaming: false,
    failure: null,
    lastMessage: message,
    stored: false,
  };
}

/** A piece of the answer: it grows the assistant's message, the first one starts it. */
function delta(state: ChatState, text: string): ChatState {
  const last = state.messages.at(-1);
  if (state.streaming && last?.role === 'assistant') {
    return {
      ...state,
      messages: [...state.messages.slice(0, -1), { role: 'assistant', text: last.text + text }],
    };
  }
  return { ...state, streaming: true, messages: [...state.messages, { role: 'assistant', text }] };
}

/** A tool call starts: its line goes after the messages so far, and the next text starts a new one. */
function tool(state: ChatState, line: string): ChatState {
  return { ...state, messages: [...state.messages, { role: 'tool', text: line }] };
}

/** The answer ends: whole, stopped by the user, or with the agent's reason. */
function ended(state: ChatState, failure: Failure | null): ChatState {
  return { ...state, running: false, streaming: false, failure };
}

/** The reducer of the chat page: every change of what the window shows goes through it. */
export function chatReducer(state: ChatState, action: ChatAction): ChatState {
  switch (action.type) {
    case 'listed':
      return { ...state, conversations: action.conversations, notice: null };
    case 'notice':
      return { ...state, notice: action.notice };
    case 'new-chat':
      return { ...state, ...blankThread, active: '' };
    case 'opening':
      return { ...state, ...blankThread, active: action.conversation, loading: true };
    case 'opened':
      // A slow read of a conversation the user has left changes nothing.
      return state.active === action.conversation
        ? { ...state, messages: action.messages, loading: false }
        : state;
    case 'open-failed':
      return state.active === action.conversation
        ? { ...state, loading: false, loadError: action.error }
        : state;
    case 'sending':
      return sending(state, action.message);
    case 'send-refused':
      return ended(state, { kind: 'reason', reason: action.reason });
    case 'started':
      return { ...state, active: action.conversation, stored: true };
    case 'delta':
      return state.running ? delta(state, action.text) : state;
    case 'tool':
      return state.running ? tool(state, action.line) : state;
    case 'done':
      return ended(state, action.stopped ? { kind: 'stopped' } : null);
    case 'failed':
      return ended(state, { kind: 'reason', reason: action.reason });
  }
}
