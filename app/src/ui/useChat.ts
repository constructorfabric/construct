import { useCallback, useEffect, useReducer, useRef } from 'react';
import { deleteAll } from '../chat-protocol';
import type { ChatEvent } from '../chat-protocol';
import { chatReducer, initialChatState } from './chat-state';
import type { ChatState } from './chat-state';

export interface Chat {
  readonly state: ChatState;
  readonly newChat: () => void;
  readonly open: (conversation: string) => void;
  /** Whether the message went out: not while an answer runs or a conversation is read. */
  readonly send: (message: string) => boolean;
  /** Sends the last message again, after an answer that failed or was stopped. */
  readonly retry: () => void;
  readonly stop: () => void;
  readonly remove: (conversation: string) => void;
  readonly removeAll: () => void;
}

/**
 * The chat page's state and what it does with the agent. `visible` is whether the page is on
 * screen: leaving it ends a running answer, which closes its connection.
 */
export function useChat(visible: boolean): Chat {
  const [state, dispatch] = useReducer(chatReducer, initialChatState);
  // What the callbacks below read when they run, so they stay the same function.
  const latest = useRef(state);
  latest.current = state;

  const refresh = useCallback(async () => {
    const result = await window.constructAgent.listConversations();
    dispatch(
      result.ok
        ? { type: 'listed', conversations: result.conversations }
        : { type: 'notice', notice: result.error },
    );
  }, []);

  useEffect(() => {
    void refresh();
    return window.constructAgent.onChatEvent((event: ChatEvent) => {
      switch (event.event) {
        case 'start':
          dispatch({ type: 'started', conversation: event.conversation });
          break;
        case 'delta':
          dispatch({ type: 'delta', text: event.text });
          break;
        case 'done':
          dispatch({ type: 'done', stopped: event.stopped });
          void refresh();
          break;
        case 'error':
          dispatch({ type: 'failed', reason: event.reason });
          void refresh();
          break;
      }
    });
  }, [refresh]);

  useEffect(() => {
    if (!visible) {
      void window.constructAgent.stopAnswer();
    }
  }, [visible]);
  useEffect(
    () => () => {
      void window.constructAgent.stopAnswer();
    },
    [],
  );

  const send = useCallback((message: string): boolean => {
    if (latest.current.running || latest.current.loading || message.trim() === '') {
      return false;
    }
    dispatch({ type: 'sending', message });
    void window.constructAgent
      .sendMessage({ conversation: latest.current.active, message })
      .then((result) => {
        if (!result.ok) {
          dispatch({ type: 'send-refused', reason: result.error });
        }
      });
    return true;
  }, []);

  const open = useCallback((conversation: string) => {
    // One read at a time: a second one could land after the user has sent a message.
    if (latest.current.running || latest.current.loading) {
      return;
    }
    dispatch({ type: 'opening', conversation });
    void window.constructAgent.getConversation(conversation).then((result) => {
      dispatch(
        result.ok
          ? { type: 'opened', conversation, messages: result.messages }
          : { type: 'open-failed', conversation, error: result.error },
      );
    });
  }, []);

  const remove = useCallback(
    (which: string) => {
      if (latest.current.running) {
        return;
      }
      void window.constructAgent.deleteConversation(which).then((result) => {
        if (!result.ok) {
          dispatch({ type: 'notice', notice: result.error });
          return;
        }
        if (which === deleteAll || which === latest.current.active) {
          dispatch({ type: 'new-chat' });
        }
        void refresh();
      });
    },
    [refresh],
  );

  return {
    state,
    newChat: useCallback(() => {
      if (!latest.current.running) {
        dispatch({ type: 'new-chat' });
      }
    }, []),
    open,
    send,
    retry: useCallback(() => {
      send(latest.current.lastMessage);
    }, [send]),
    stop: useCallback(() => {
      void window.constructAgent.stopAnswer();
    }, []),
    remove,
    removeAll: useCallback(() => {
      remove(deleteAll);
    }, [remove]),
  };
}
