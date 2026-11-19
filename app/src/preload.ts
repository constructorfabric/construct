import { contextBridge, ipcRenderer } from 'electron';
import type { IpcRendererEvent } from 'electron';
import {
  SETTINGS_COLLECTION_CHANNEL,
  SETTINGS_GET_CHANNEL,
  SETTINGS_REMOVE_KEY_CHANNEL,
  SETTINGS_SAVE_CHANNEL,
  SETTINGS_TEST_CHANNEL,
  SETTINGS_TEST_PROGRESS_CHANNEL,
} from './agent-settings';
import type { ModelTestResult, SettingsResult, TestProgress } from './agent-settings';
import {
  DASHBOARD_DOCUMENT_CHANNEL,
  DASHBOARD_NUMBERS_CHANNEL,
  DASHBOARD_PROJECT_CHANNEL,
  DASHBOARD_SESSIONS_CHANNEL,
  DASHBOARD_SUMMARY_CHANNEL,
} from './dashboard-protocol';
import type {
  DashboardResult,
  DocumentResult,
  SessionsResult,
  SummaryResult,
} from './dashboard-protocol';
import {
  SETUP_CANCEL_CHANNEL,
  SETUP_START_CHANNEL,
  SETUP_STATUS_CHANNEL,
} from './dashboard-setup-protocol';
import type { SetupResult } from './dashboard-setup-protocol';
import { COPY_TEXT_CHANNEL, OPEN_LINK_CHANNEL, OPEN_PANE_CHANNEL } from './external';
import {
  MEMORY_DELETE_CHANNEL,
  MEMORY_ERASE_CHANNEL,
  MEMORY_ERASING_CHANNEL,
  MEMORY_FACTS_CHANNEL,
  MEMORY_VERSION_CHANNEL,
} from './memory-protocol';
import type {
  EraseResult,
  ErasingResult,
  FactDeleteResult,
  FactsResult,
  VersionResult,
} from './memory-protocol';
import { NEEDS_MOVE_CHANNEL, STATUS_CHANNEL } from './agent-status';
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
  getNeedsMove: () => ipcRenderer.invoke(NEEDS_MOVE_CHANNEL) as Promise<boolean>,
  getSettings: () => ipcRenderer.invoke(SETTINGS_GET_CHANNEL) as Promise<SettingsResult>,
  saveModel: (input) => ipcRenderer.invoke(SETTINGS_SAVE_CHANNEL, input) as Promise<SettingsResult>,
  testModel: (input) =>
    ipcRenderer.invoke(SETTINGS_TEST_CHANNEL, input) as Promise<ModelTestResult>,
  onTestProgress: (listener) => {
    // Only the progress goes through, never Electron's own event object.
    const handler = (_event: IpcRendererEvent, progress: TestProgress): void => {
      listener(progress);
    };
    ipcRenderer.on(SETTINGS_TEST_PROGRESS_CHANNEL, handler);
    return () => {
      ipcRenderer.removeListener(SETTINGS_TEST_PROGRESS_CHANNEL, handler);
    };
  },
  removeKey: () => ipcRenderer.invoke(SETTINGS_REMOVE_KEY_CHANNEL) as Promise<SettingsResult>,
  listConversations: () => ipcRenderer.invoke(CHAT_LIST_CHANNEL) as Promise<ListResult>,
  getConversation: (conversation) =>
    ipcRenderer.invoke(CHAT_GET_CHANNEL, conversation) as Promise<GetResult>,
  deleteConversation: (which) =>
    ipcRenderer.invoke(CHAT_DELETE_CHANNEL, which) as Promise<DeleteResult>,
  sendMessage: (input) => ipcRenderer.invoke(CHAT_SEND_CHANNEL, input) as Promise<SendResult>,
  stopAnswer: () => ipcRenderer.invoke(CHAT_STOP_CHANNEL) as Promise<void>,
  getDashboardNumbers: (input) =>
    ipcRenderer.invoke(DASHBOARD_NUMBERS_CHANNEL, input) as Promise<DashboardResult>,
  getDashboardDocument: () =>
    ipcRenderer.invoke(DASHBOARD_DOCUMENT_CHANNEL) as Promise<DocumentResult>,
  changeDashboardProject: (change) =>
    ipcRenderer.invoke(DASHBOARD_PROJECT_CHANNEL, change) as Promise<DocumentResult>,
  getDashboardSessions: (input) =>
    ipcRenderer.invoke(DASHBOARD_SESSIONS_CHANNEL, input) as Promise<SessionsResult>,
  getDashboardSummary: (input) =>
    ipcRenderer.invoke(DASHBOARD_SUMMARY_CHANNEL, input) as Promise<SummaryResult>,
  getMemoryFacts: () => ipcRenderer.invoke(MEMORY_FACTS_CHANNEL) as Promise<FactsResult>,
  getMemoryVersion: () => ipcRenderer.invoke(MEMORY_VERSION_CHANNEL) as Promise<VersionResult>,
  deleteMemoryFact: (factId: string) =>
    ipcRenderer.invoke(MEMORY_DELETE_CHANNEL, factId) as Promise<FactDeleteResult>,
  eraseEverything: () => ipcRenderer.invoke(MEMORY_ERASE_CHANNEL) as Promise<EraseResult>,
  getErasing: () => ipcRenderer.invoke(MEMORY_ERASING_CHANNEL) as Promise<ErasingResult>,
  startDashboardSetup: () => ipcRenderer.invoke(SETUP_START_CHANNEL) as Promise<SetupResult>,
  getDashboardSetup: () => ipcRenderer.invoke(SETUP_STATUS_CHANNEL) as Promise<SetupResult>,
  cancelDashboardSetup: () => ipcRenderer.invoke(SETUP_CANCEL_CHANNEL) as Promise<SetupResult>,
  openLink: (url) => ipcRenderer.invoke(OPEN_LINK_CHANNEL, url) as Promise<boolean>,
  openPane: (pane) => ipcRenderer.invoke(OPEN_PANE_CHANNEL, pane) as Promise<boolean>,
  copyText: (text) => ipcRenderer.invoke(COPY_TEXT_CHANNEL, text) as Promise<boolean>,
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
  saveCollection: (input) =>
    ipcRenderer.invoke(SETTINGS_COLLECTION_CHANNEL, input) as Promise<SettingsResult>,
};

contextBridge.exposeInMainWorld('constructAgent', api);
