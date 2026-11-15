import { contextBridge, ipcRenderer } from 'electron';
import type { IpcRendererEvent } from 'electron';
import {
  SETTINGS_GET_CHANNEL,
  SETTINGS_REMOVE_KEY_CHANNEL,
  SETTINGS_SAVE_CHANNEL,
  SETTINGS_TEST_CHANNEL,
} from './agent-settings';
import type { ModelTestResult, SettingsResult } from './agent-settings';
import { OPEN_LINK_CHANNEL } from './external';
import { STATUS_CHANNEL } from './agent-status';
import type { AgentStatus, ConstructAgentApi } from './agent-status';
import {
  CHAT_DELETE_CHANNEL,
  CHAT_EVENT_CHANNEL,
  CHAT_GET_CHANNEL,
  CHAT_LIST_CHANNEL,
  CHAT_SEND_CHANNEL,
  CHAT_STOP_CHANNEL,
} from './chat-protocol';
import type { ChatEvent, DeleteResult, GetResult, ListResult, SendResult } from './chat-protocol';

const api: ConstructAgentApi = {
  getStatus: () => ipcRenderer.invoke(STATUS_CHANNEL) as Promise<AgentStatus>,
  getSettings: () => ipcRenderer.invoke(SETTINGS_GET_CHANNEL) as Promise<SettingsResult>,
  saveModel: (input) => ipcRenderer.invoke(SETTINGS_SAVE_CHANNEL, input) as Promise<SettingsResult>,
  testModel: (input) =>
    ipcRenderer.invoke(SETTINGS_TEST_CHANNEL, input) as Promise<ModelTestResult>,
  removeKey: () => ipcRenderer.invoke(SETTINGS_REMOVE_KEY_CHANNEL) as Promise<SettingsResult>,
  listConversations: () => ipcRenderer.invoke(CHAT_LIST_CHANNEL) as Promise<ListResult>,
  getConversation: (conversation) =>
    ipcRenderer.invoke(CHAT_GET_CHANNEL, conversation) as Promise<GetResult>,
  deleteConversation: (which) =>
    ipcRenderer.invoke(CHAT_DELETE_CHANNEL, which) as Promise<DeleteResult>,
  sendMessage: (input) => ipcRenderer.invoke(CHAT_SEND_CHANNEL, input) as Promise<SendResult>,
  stopAnswer: () => ipcRenderer.invoke(CHAT_STOP_CHANNEL) as Promise<void>,
  openLink: (url) => ipcRenderer.invoke(OPEN_LINK_CHANNEL, url) as Promise<boolean>,
  onChatEvent: (listener) => {
    // Only the event goes through, never Electron's own event object.
    const handler = (_event: IpcRendererEvent, chatEvent: ChatEvent): void => {
      listener(chatEvent);
    };
    ipcRenderer.on(CHAT_EVENT_CHANNEL, handler);
    return () => {
      ipcRenderer.removeListener(CHAT_EVENT_CHANNEL, handler);
    };
  },
};

contextBridge.exposeInMainWorld('constructAgent', api);
