import { useEffect, useState } from 'react';
import type { AgentStatus } from '../agent-status';
import { startStatusPoller } from '../status-poller';

/** The agent's latest status; `null` until the first answer. */
export function useAgentStatus(): AgentStatus | null {
  const [status, setStatus] = useState<AgentStatus | null>(null);
  useEffect(() => {
    const poller = startStatusPoller(() => window.constructAgent.getStatus(), setStatus);
    return poller.stop;
  }, []);
  return status;
}
