import type { ModelSettingsApi } from './agent-settings';
import type { ChatApi } from './chat-protocol';
import type { DashboardApi } from './dashboard-protocol';
import type { DashboardSetupApi } from './dashboard-setup-protocol';
import type { ExternalApi } from './external';
import type { MemoryApi } from './memory-protocol';
import { parseGrants } from './screen-grants';
import type { Grant } from './screen-grants';

/**
 * Whether the agent starts at login. `unknown` when the agent is not running or its reply has no
 * such field; `not_supported` where the app has no way to register it (Windows).
 */
export type StartAtLogin = 'registered' | 'not_registered' | 'unknown' | 'not_supported';

/** The state of a part the agent runs: its database or the Construct gear. */
export type PartState = 'ready' | 'starting' | 'failed';

export const partStates: readonly PartState[] = ['ready', 'starting', 'failed'];

export interface Part {
  readonly state: PartState;
  /** Why the part is not ready, a short fixed text of the agent; never sent for a ready one. */
  readonly reason?: string;
}

export type AgentStatus =
  | {
      readonly state: 'running';
      readonly version: string;
      readonly pid: number;
      /** The model's name as the agent sends it (`not set` when none is configured). */
      readonly model?: string;
      readonly startAtLogin: StartAtLogin;
      /** Absent when the agent does not tell it, or tells it in a way the window does not know. */
      readonly database?: Part;
      readonly construct?: Part;
      /** The permissions of the apps and websites collection; absent when the agent sends none. */
      readonly grants?: readonly Grant[];
    }
  /** The socket answered "nobody here": no socket file, or nothing listening on it. */
  | { readonly state: 'stopped'; readonly reason: string; readonly startAtLogin: StartAtLogin }
  /** The app could not find out: a local failure, or an agent that answered wrongly. */
  | { readonly state: 'unknown'; readonly reason: string; readonly startAtLogin: StartAtLogin };

export type ConstructAgentApi = ModelSettingsApi &
  ChatApi &
  DashboardApi &
  DashboardSetupApi &
  ExternalApi &
  MemoryApi & {
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
  if ('model' in reply && typeof reply.model !== 'string') {
    throw new Error('the agent reply has a model that is not text');
  }
  const database = 'database' in reply ? parsePart(reply.database) : undefined;
  const construct = 'construct' in reply ? parsePart(reply.construct) : undefined;
  const grants = 'screen' in reply ? parseGrants(reply.screen) : undefined;
  return {
    state: 'running',
    version: reply.version,
    pid: reply.pid,
    ...('model' in reply && typeof reply.model === 'string' ? { model: reply.model } : {}),
    startAtLogin,
    ...(database === undefined ? {} : { database }),
    ...(construct === undefined ? {} : { construct }),
    ...(grants === undefined ? {} : { grants }),
  };
}

function isPartState(value: unknown): value is PartState {
  return partStates.some((state) => state === value);
}

/**
 * The `database` or `construct` field: `{"state": ..., "reason": ...}`, or the plain state string
 * an older agent sends. What the window does not know (another state, another shape) is skipped,
 * not an error: the rest of the status still shows. A reason is kept only for a part that is not
 * ready.
 */
export function parsePart(value: unknown): Part | undefined {
  if (isPartState(value)) {
    return { state: value };
  }
  if (typeof value !== 'object' || value === null || !('state' in value)) {
    return undefined;
  }
  if (!isPartState(value.state)) {
    return undefined;
  }
  if (value.state === 'ready' || !('reason' in value) || typeof value.reason !== 'string') {
    return { state: value.state };
  }
  const reason = value.reason.trim();
  return reason === '' ? { state: value.state } : { state: value.state, reason };
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
