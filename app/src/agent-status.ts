import type { ModelSettingsApi } from './agent-settings';
import type { ChatApi } from './chat-protocol';
import type { ExternalApi } from './external';

/**
 * Whether the agent starts at login. `unknown` when the agent is not running or its reply has no
 * such field; `not_supported` where the app has no way to register it (Windows).
 */
export type StartAtLogin = 'registered' | 'not_registered' | 'unknown' | 'not_supported';

export type AgentStatus =
  | {
      readonly state: 'running';
      readonly version: string;
      readonly pid: number;
      /** The model's name as the agent sends it (`not set` when none is configured). */
      readonly model?: string;
      readonly startAtLogin: StartAtLogin;
    }
  /** The socket answered "nobody here": no socket file, or nothing listening on it. */
  | { readonly state: 'stopped'; readonly reason: string; readonly startAtLogin: StartAtLogin }
  /** The app could not find out: a local failure, or an agent that answered wrongly. */
  | { readonly state: 'unknown'; readonly reason: string; readonly startAtLogin: StartAtLogin };

export type ConstructAgentApi = ModelSettingsApi &
  ChatApi &
  ExternalApi & {
    readonly getStatus: () => Promise<AgentStatus>;
    /** `true` when the app runs from a place macOS will remove, so no login service was set up. */
    readonly getNeedsMove: () => Promise<boolean>;
  };

export const STATUS_CHANNEL = 'agent:status';
export const NEEDS_MOVE_CHANNEL = 'agent:needs-move';

/** Turns the agent's JSON reply line into a status; anything else is an error. */
export function parseStatusReply(line: string): AgentStatus {
  const reply: unknown = JSON.parse(line);
  if (typeof reply !== 'object' || reply === null) {
    throw new Error('the agent reply is not a JSON object');
  }
  if (!('status' in reply) || reply.status !== 'running') {
    throw new Error(`the agent did not report running: ${line}`);
  }
  if (!('version' in reply) || typeof reply.version !== 'string') {
    throw new Error('the agent reply has no version');
  }
  if (!('pid' in reply) || typeof reply.pid !== 'number') {
    throw new Error('the agent reply has no pid');
  }
  const startAtLogin = parseService(reply);
  if (!('model' in reply)) {
    return { state: 'running', version: reply.version, pid: reply.pid, startAtLogin };
  }
  if (typeof reply.model !== 'string') {
    throw new Error('the agent reply has a model that is not text');
  }
  return {
    state: 'running',
    version: reply.version,
    pid: reply.pid,
    model: reply.model,
    startAtLogin,
  };
}

/** The optional `service` field: an agent that does not send it is older, not wrong. */
function parseService(reply: object): StartAtLogin {
  if (!('service' in reply)) {
    return 'unknown';
  }
  if (reply.service === 'registered' || reply.service === 'not_registered') {
    return reply.service;
  }
  throw new Error('the agent reply has a service that is neither registered nor not_registered');
}
