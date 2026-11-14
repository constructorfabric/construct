import { contextBridge, ipcRenderer } from 'electron';
import { STATUS_CHANNEL } from './agent-status';
import type { AgentStatus, ConstructAgentApi } from './agent-status';

const api: ConstructAgentApi = {
  getStatus: () => ipcRenderer.invoke(STATUS_CHANNEL) as Promise<AgentStatus>,
};

contextBridge.exposeInMainWorld('constructAgent', api);
