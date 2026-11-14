import './index.css';
import type { AgentStatus } from './agent-status';

const refreshMs = 3000;

function requireElement(id: string): HTMLElement {
  const element = document.getElementById(id);
  if (element === null) {
    throw new Error(`the status window markup has no #${id}`);
  }
  return element;
}

const statusElement = requireElement('agent-status');
const detailElement = requireElement('agent-detail');

function show(status: AgentStatus): void {
  statusElement.dataset.state = status.state;
  statusElement.textContent = status.state;
  detailElement.textContent =
    status.state === 'running' ? `version ${status.version}, pid ${status.pid}` : status.reason;
}

let inFlight = false;

// A poll starts only when the previous one has finished; the main process bounds each one.
async function refresh(): Promise<void> {
  if (inFlight) {
    return;
  }
  inFlight = true;
  try {
    show(await window.constructAgent.getStatus());
  } catch (error) {
    show({ state: 'unknown', reason: error instanceof Error ? error.message : String(error) });
  } finally {
    inFlight = false;
  }
}

function refreshInBackground(): void {
  refresh().catch((error: unknown) => {
    detailElement.textContent = error instanceof Error ? error.message : String(error);
  });
}

refreshInBackground();
const timer = setInterval(refreshInBackground, refreshMs);
window.addEventListener('beforeunload', () => {
  clearInterval(timer);
});
