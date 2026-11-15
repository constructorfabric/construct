import { contextBridge, ipcRenderer } from 'electron';
import {
  SETTINGS_GET_CHANNEL,
  SETTINGS_REMOVE_KEY_CHANNEL,
  SETTINGS_SAVE_CHANNEL,
  SETTINGS_TEST_CHANNEL,
} from './agent-settings';
import type { ModelTestResult, SettingsResult } from './agent-settings';
import { STATUS_CHANNEL } from './agent-status';
import type { AgentStatus, ConstructAgentApi } from './agent-status';

const api: ConstructAgentApi = {
  getStatus: () => ipcRenderer.invoke(STATUS_CHANNEL) as Promise<AgentStatus>,
  getSettings: () => ipcRenderer.invoke(SETTINGS_GET_CHANNEL) as Promise<SettingsResult>,
  saveModel: (input) => ipcRenderer.invoke(SETTINGS_SAVE_CHANNEL, input) as Promise<SettingsResult>,
  testModel: (input) =>
    ipcRenderer.invoke(SETTINGS_TEST_CHANNEL, input) as Promise<ModelTestResult>,
  removeKey: () => ipcRenderer.invoke(SETTINGS_REMOVE_KEY_CHANNEL) as Promise<SettingsResult>,
};

contextBridge.exposeInMainWorld('constructAgent', api);
