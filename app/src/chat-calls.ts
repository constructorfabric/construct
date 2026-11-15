import {
  AgentSilentError,
  agentSocketPath,
  callAgent,
  callAgentLines,
  chatReadLimits,
  chatStreamLimits,
  failureText,
  isAgentAbsent,
  settingsLimits,
  streamAgent,
} from './agent-socket';
import type { AgentStream, StreamLimits, Where } from './agent-socket';
import {
  chatSendLine,
  deleteAll,
  isConversationId,
  parseChatEvent,
  parseChatInput,
  parseDeleteReply,
  parseGetLines,
  parseListReply,
  reasonAgentAbsent,
  reasonUnreadable,
} from './chat-protocol';
import type { ChatEvent, DeleteResult, GetResult, ListResult, SendResult } from './chat-protocol';

const invalidInput = 'the window sent a chat request in a form the app does not accept';
const busy = 'busy';
/** After `chat-stop` the agent ends the answer at once; this is how long the app waits for it. */
const stopGraceMs = 1500;

export interface ChatHandlers {
  readonly list: () => Promise<ListResult>;
  readonly get: (value: unknown) => Promise<GetResult>;
  readonly remove: (value: unknown) => Promise<DeleteResult>;
  readonly send: (value: unknown) => SendResult;
  readonly stop: () => Promise<void>;
  /** Closes the socket of the running answer, when the window goes away. */
  readonly close: () => void;
}

interface Running {
  stream: AgentStream;
  conversation: string;
  readonly guard: { timer: ReturnType<typeof setTimeout> | undefined };
}

/**
 * What the chat channels do. The window's input is checked here, since the window is not trusted.
 * One answer runs at a time, on one connection: its lines go to `emit` as they arrive, and the
 * window never sees the socket. Nothing here logs a message or an answer.
 */
export function createChatHandlers(
  where: Where,
  emit: (event: ChatEvent) => void,
  limits: { readonly stream: StreamLimits; readonly stopGraceMs: number } = {
    stream: chatStreamLimits,
    stopGraceMs,
  },
): ChatHandlers {
  let running: Running | null = null;

  const end = (current: Running, event: ChatEvent): void => {
    if (running !== current) {
      return;
    }
    clearTimeout(current.guard.timer);
    running = null;
    current.stream.close();
    emit(event);
  };

  return {
    list: async () => {
      try {
        const lines = await callAgentLines(where.env, where.platform, 'chat-list', chatReadLimits);
        return parseListReply(lines[0] ?? '');
      } catch (error) {
        return { ok: false, error: failureText(error) };
      }
    },
    get: async (value) => {
      if (!isConversationId(value)) {
        return { ok: false, error: invalidInput };
      }
      try {
        return parseGetLines(
          await callAgentLines(where.env, where.platform, `chat-get ${value}`, chatReadLimits),
        );
      } catch (error) {
        return { ok: false, error: failureText(error) };
      }
    },
    remove: async (value) => {
      if (value !== deleteAll && !isConversationId(value)) {
        return { ok: false, error: invalidInput };
      }
      try {
        return parseDeleteReply(
          await callAgent(where.env, where.platform, `chat-delete ${value}`, settingsLimits),
        );
      } catch (error) {
        return { ok: false, error: failureText(error) };
      }
    },
    send: (value) => {
      const input = parseChatInput(value);
      if (input === null) {
        return { ok: false, error: invalidInput };
      }
      if (running !== null) {
        return { ok: false, error: busy };
      }
      let path: string;
      try {
        path = agentSocketPath(where.env, where.platform);
      } catch (error) {
        return { ok: false, error: failureText(error) };
      }
      const current: Running = {
        stream: { close: () => undefined },
        conversation: input.conversation,
        guard: { timer: undefined },
      };
      // The stream is made first and put in `current` after: its callbacks run later, never inside
      // this call, so `current` is complete by then.
      const stream = streamAgent(
        path,
        chatSendLine(input),
        limits.stream,
        (line) => {
          let event: ChatEvent;
          try {
            event = parseChatEvent(line);
          } catch {
            end(current, { event: 'error', reason: reasonUnreadable });
            return;
          }
          if (event.event === 'start') {
            current.conversation = event.conversation;
            emit(event);
          } else if (event.event === 'delta') {
            emit(event);
          } else {
            end(current, event);
          }
        },
        (error) => {
          // The agent closed the socket, or the call failed, before it said how the answer ended.
          let reason = 'cut';
          if (error instanceof AgentSilentError) {
            reason = 'timeout';
          } else if (isAgentAbsent(error)) {
            reason = reasonAgentAbsent;
          }
          end(current, { event: 'error', reason });
        },
      );
      current.stream = stream;
      running = current;
      return { ok: true };
    },
    stop: async () => {
      const current = running;
      if (current === null) {
        return;
      }
      // The answer ends with the agent's `done`, which comes after the part is stored. When it does
      // not come soon, the socket is closed, which the agent also takes as a stop.
      current.guard.timer ??= setTimeout(() => {
        end(current, { event: 'done', stopped: true });
      }, limits.stopGraceMs);
      if (current.conversation === '') {
        end(current, { event: 'done', stopped: true });
        return;
      }
      try {
        await callAgent(
          where.env,
          where.platform,
          `chat-stop ${current.conversation}`,
          settingsLimits,
        );
      } catch {
        // The guard above ends the answer.
      }
    },
    close: () => {
      if (running !== null) {
        clearTimeout(running.guard.timer);
        running.stream.close();
        running = null;
      }
    },
  };
}
