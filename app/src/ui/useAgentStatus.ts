import { useCallback, useEffect, useRef, useState } from 'react';
import type { AgentStatus } from '../agent-status';
import { startStatusPoller } from '../status-poller';
import type { StatusPoller } from '../status-poller';

/** The agent's latest status, `null` until the first answer, and a way to ask again now. */
export function useAgentStatus(): { status: AgentStatus | null; refresh: () => void } {
  const [status, setStatus] = useState<AgentStatus | null>(null);
  const poller = useRef<StatusPoller | null>(null);
  useEffect(() => {
    const started = startStatusPoller(() => window.constructAgent.getStatus(), setStatus);
    poller.current = started;
    return started.stop;
  }, []);
  const refresh = useCallback(() => {
    poller.current?.refresh();
  }, []);
  return { status, refresh };
}
