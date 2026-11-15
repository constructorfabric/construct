import { isRecord, parseLine, replyError } from './agent-json';

/** One conversation of the sidebar. */
export interface ConversationSummary {
  readonly id: string;
  /** The first message, cut by the agent; it never holds more than the user typed. */
  readonly title: string;
  readonly time: string;
}

/** A message of the thread. The agent also stores tool records; the window never sees them. */
export interface ChatMessage {
  readonly role: 'user' | 'assistant';
  readonly text: string;
}

/** What the agent says while it answers, line by line. `reason` is one of its fixed texts. */
export type ChatEvent =
  | { readonly event: 'start'; readonly conversation: string }
  | { readonly event: 'delta'; readonly text: string }
  | { readonly event: 'done'; readonly stopped: boolean }
  | { readonly event: 'error'; readonly reason: string };

export interface ChatInput {
  /** An id, or empty for a new conversation. */
  readonly conversation: string;
  readonly message: string;
}

export type ListResult =
  | { readonly ok: true; readonly conversations: readonly ConversationSummary[] }
  | { readonly ok: false; readonly error: string };

export type GetResult =
  | { readonly ok: true; readonly messages: readonly ChatMessage[] }
  | { readonly ok: false; readonly error: string };

export type DeleteResult = { readonly ok: true } | { readonly ok: false; readonly error: string };

/** Whether an answer was started. Its events come through `onChatEvent`. */
export type SendResult = { readonly ok: true } | { readonly ok: false; readonly error: string };

export interface ChatApi {
  readonly listConversations: () => Promise<ListResult>;
  readonly getConversation: (conversation: string) => Promise<GetResult>;
  /** One conversation, or `all`. */
  readonly deleteConversation: (which: string) => Promise<DeleteResult>;
  readonly sendMessage: (input: ChatInput) => Promise<SendResult>;
  /** Asks the agent to end the running answer; the end comes as an event. */
  readonly stopAnswer: () => Promise<void>;
  /** Returns the function that removes the listener. */
  readonly onChatEvent: (listener: (event: ChatEvent) => void) => () => void;
}

export const CHAT_LIST_CHANNEL = 'chat:list';
export const CHAT_GET_CHANNEL = 'chat:get';
export const CHAT_DELETE_CHANNEL = 'chat:delete';
export const CHAT_SEND_CHANNEL = 'chat:send';
export const CHAT_STOP_CHANNEL = 'chat:stop';
export const CHAT_EVENT_CHANNEL = 'chat:event';

/** What the app says itself, next to the agent's reasons. */
export const reasonAgentAbsent = 'agent not running';
export const reasonUnreadable = 'unreadable answer';

/** The `all` that `chat-delete` takes; a conversation id is a UUID, so the two never meet. */
export const deleteAll = 'all';

const conversationId = /^[0-9a-f-]{1,64}$/i;

/** The input of a bridge call, which comes from the window and so is checked. */
export function parseChatInput(value: unknown): ChatInput | null {
  if (!isRecord(value)) {
    return null;
  }
  const { conversation, message } = value;
  if (typeof conversation !== 'string' || typeof message !== 'string') {
    return null;
  }
  if (conversation !== '' && !conversationId.test(conversation)) {
    return null;
  }
  return { conversation, message };
}

/** Whether `value`, which comes from the window, can go into a `chat-get` or `chat-delete` line. */
export function isConversationId(value: unknown): value is string {
  return typeof value === 'string' && conversationId.test(value);
}

/** `chat-send {...}`; the message is JSON-encoded, so it cannot end the line. */
export function chatSendLine(input: ChatInput): string {
  return `chat-send ${JSON.stringify({ conversation: input.conversation, message: input.message })}`;
}

/** One line of a `chat-send` answer; anything else is an error. */
export function parseChatEvent(line: string): ChatEvent {
  const reply = parseLine(line);
  switch (reply.event) {
    case 'start':
      if (typeof reply.conversation !== 'string') {
        throw new Error('the agent event has no conversation');
      }
      return { event: 'start', conversation: reply.conversation };
    case 'delta':
      if (typeof reply.text !== 'string') {
        throw new Error('the agent event has no text');
      }
      return { event: 'delta', text: reply.text };
    case 'done':
      return { event: 'done', stopped: reply.stopped === true };
    case 'error':
      if (typeof reply.reason !== 'string') {
        throw new Error('the agent event has no reason');
      }
      return { event: 'error', reason: reply.reason };
    default:
      throw new Error('the agent sent an unknown event');
  }
}

/** Turns a `chat-list` reply into a result. The agent sends the newest first. */
export function parseListReply(line: string): ListResult {
  const reply = parseLine(line);
  const error = replyError(reply);
  if (error !== null) {
    return { ok: false, error };
  }
  if (!Array.isArray(reply.conversations)) {
    throw new Error('the agent reply has no conversations');
  }
  const conversations = (reply.conversations as unknown[]).map((entry): ConversationSummary => {
    if (
      !isRecord(entry) ||
      typeof entry.conversation !== 'string' ||
      typeof entry.title !== 'string' ||
      typeof entry.time !== 'string'
    ) {
      throw new Error('the agent listed a conversation in a form the app does not accept');
    }
    return { id: entry.conversation, title: entry.title, time: entry.time };
  });
  return { ok: true, conversations };
}

/** Turns the lines of `chat-get` into a result: the turns, without the tool records. */
export function parseGetLines(lines: readonly string[]): GetResult {
  const messages: ChatMessage[] = [];
  for (const line of lines) {
    const record = parseLine(line);
    const error = replyError(record);
    if (error !== null) {
      return { ok: false, error };
    }
    if (record.kind !== 'turn') {
      continue;
    }
    if (
      (record.role !== 'user' && record.role !== 'assistant') ||
      typeof record.text !== 'string'
    ) {
      throw new Error('the agent sent a turn in a form the app does not accept');
    }
    messages.push({ role: record.role, text: record.text });
  }
  return { ok: true, messages };
}

/** Turns a `chat-delete` reply into a result. */
export function parseDeleteReply(line: string): DeleteResult {
  const reply = parseLine(line);
  const error = replyError(reply);
  if (error !== null) {
    return { ok: false, error };
  }
  if (typeof reply.deleted !== 'number') {
    throw new Error('the agent reply has no count');
  }
  return { ok: true };
}
