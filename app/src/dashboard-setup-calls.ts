import { callAgent, failureText, setupLimits } from './agent-socket';
import type { Where } from './agent-socket';
import {
  parseSetupReply,
  setupCancelLine,
  setupStartLine,
  setupStatusLine,
} from './dashboard-setup-protocol';
import type { SetupResult } from './dashboard-setup-protocol';

export interface DashboardSetupHandlers {
  readonly start: () => Promise<SetupResult>;
  readonly status: () => Promise<SetupResult>;
  readonly cancel: () => Promise<SetupResult>;
}

/** What the setup channels do: each sends one fixed command, so the window sends no argument. */
export function createDashboardSetupHandlers(where: Where): DashboardSetupHandlers {
  const call = async (line: string): Promise<SetupResult> => {
    try {
      return parseSetupReply(await callAgent(where.env, where.platform, line, setupLimits));
    } catch (error) {
      return { ok: false, error: failureText(error) };
    }
  };
  return {
    start: () => call(setupStartLine),
    status: () => call(setupStatusLine),
    cancel: () => call(setupCancelLine),
  };
}
