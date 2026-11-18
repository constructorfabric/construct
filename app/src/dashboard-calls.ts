import { callAgent, dashboardLimits, failureText } from './agent-socket';
import type { Where } from './agent-socket';
import {
  dashboardGetLine,
  dashboardNumbersLine,
  parseDashboardInput,
  parseDashboardReply,
  parseDocumentReply,
  parseProjectChange,
  projectChangeLine,
} from './dashboard-protocol';
import type { DashboardResult, DocumentResult } from './dashboard-protocol';

const invalidInput = 'the window sent a dashboard request in a form the app does not accept';
const invalidChange = 'the window sent a project change in a form the app does not accept';

export interface DashboardHandlers {
  readonly numbers: (value: unknown) => Promise<DashboardResult>;
  readonly document: () => Promise<DocumentResult>;
  readonly project: (value: unknown) => Promise<DocumentResult>;
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
    document: async () => {
      try {
        return parseDocumentReply(
          await callAgent(where.env, where.platform, dashboardGetLine, dashboardLimits),
        );
      } catch (error) {
        return { ok: false, error: failureText(error) };
      }
    },
    project: async (value) => {
      const change = parseProjectChange(value);
      if (change === null) {
        return { ok: false, error: invalidChange };
      }
      try {
        return parseDocumentReply(
          await callAgent(where.env, where.platform, projectChangeLine(change), dashboardLimits),
        );
      } catch (error) {
        return { ok: false, error: failureText(error) };
      }
    },
  };
}
