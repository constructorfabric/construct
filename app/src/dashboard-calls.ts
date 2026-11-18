import { callAgent, dashboardLimits, failureText } from './agent-socket';
import type { Where } from './agent-socket';
import {
  dashboardNumbersLine,
  parseDashboardInput,
  parseDashboardReply,
} from './dashboard-protocol';
import type { DashboardResult } from './dashboard-protocol';

const invalidInput = 'the window sent a dashboard request in a form the app does not accept';

export interface DashboardHandlers {
  readonly numbers: (value: unknown) => Promise<DashboardResult>;
}

/** What the dashboard channel does. The window's input is checked here: the window is not trusted. */
export function createDashboardHandlers(where: Where): DashboardHandlers {
  return {
    numbers: async (value) => {
      const input = parseDashboardInput(value);
      if (input === null) {
        return { ok: false, error: invalidInput };
      }
      try {
        return parseDashboardReply(
          await callAgent(where.env, where.platform, dashboardNumbersLine(input), dashboardLimits),
        );
      } catch (error) {
        return { ok: false, error: failureText(error) };
      }
    },
  };
}
