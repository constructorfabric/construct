import { isCount, isRecord } from './agent-json';
import type { ModelSettingsApi } from './agent-settings';
import type { ChatApi } from './chat-protocol';
import type { DashboardApi } from './dashboard-protocol';
import type { GlasshouseApi } from './glasshouse';
import type { DashboardSetupApi } from './dashboard-setup-protocol';
import type { ExternalApi } from './external';
import type { MemoryApi } from './memory-protocol';
import { missingGrants, parseGrants } from './screen-grants';
import type { Grant, GrantName } from './screen-grants';

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

/** What a connector does: follows its tool and stores turns, reads the files for the first time, is switched off, or failed. */
export type ConnectorState = 'collecting' | 'scanning' | 'off' | 'failed';

/** One tool the agent collects from, as the status answer tells it. */
export interface Connector {
  /** The tool's name as the agent sends it, such as `claude-code`. */
  readonly name: string;
  readonly state: ConnectorState;
  /** Why the tool is not followed fully: a fixed text of the agent, shown as it comes. */
  readonly reason?: string;
  /** Turns stored since the agent started. */
  readonly turns: number;
  /** Lines that did not parse or were too long. */
  readonly skipped: number;
  /** Files that could not be read in the last pass. */
  readonly unreadable: number;
  /** When the last turn was stored, in milliseconds since the epoch; absent until one is, or from an older agent. */
  readonly lastTurnAt?: number;
}

/** What the apps and websites collection does: samples, is switched off, lacks a permission, or failed. */
export type ScreenState = 'collecting' | 'off' | 'needs_permission' | 'failed';

export const screenStates: readonly ScreenState[] = [
  'collecting',
  'off',
  'needs_permission',
  'failed',
];

/** The `screen` row of the status answer, as the Status page and the sidebar show it. */
export interface ScreenCollection {
  readonly state: ScreenState;
  /** Why the collection is off or failed: a fixed text of the agent, shown as it comes. */
  readonly reason?: string;
  /** Samples stored today. */
  readonly samplesToday: number;
  /** When the last sample was stored, in milliseconds since the epoch; absent until one is. */
  readonly lastSampleAt?: number;
  /** The permissions the person has not given; empty unless the state is `needs_permission`. */
  readonly missing: readonly GrantName[];
}

/** The reason of an `off` row on a computer that cannot collect them: there is nothing to show. */
const unsupportedReason = 'not supported on this computer';

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
      /** The apps and websites collection; absent when the agent sends none or cannot collect them. */
      readonly screen?: ScreenCollection;
      /** The tools the agent collects from; absent when the agent sends none. */
      readonly connectors?: readonly Connector[];
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
  GlasshouseApi &
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
  const screen = 'screen' in reply ? parseScreen(reply.screen) : undefined;
  const connectors = 'connectors' in reply ? parseConnectors(reply.connectors) : undefined;
  return {
    state: 'running',
    version: reply.version,
    pid: reply.pid,
    ...('model' in reply && typeof reply.model === 'string' ? { model: reply.model } : {}),
    startAtLogin,
    ...(database === undefined ? {} : { database }),
    ...(construct === undefined ? {} : { construct }),
    ...(grants === undefined ? {} : { grants }),
    ...(screen === undefined ? {} : { screen }),
    ...(connectors === undefined ? {} : { connectors }),
  };
}

/**
 * The `connectors` field: an object with one entry per tool. What the window does not know (another
 * shape, a field of another type) is skipped, not an error: the rest of the status still shows.
 */
export function parseConnectors(value: unknown): readonly Connector[] | undefined {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return undefined;
  }
  const connectors: Connector[] = [];
  for (const [name, entry] of Object.entries(value)) {
    if (!isRecord(entry) || Array.isArray(entry)) {
      continue;
    }
    // An `error` of any kind marks the tool failed; only a text of the agent's is kept as the reason.
    const failed = 'error' in entry && entry.error !== null && entry.error !== undefined;
    const reason = typeof entry.error === 'string' ? entry.error.trim() : '';
    const lastTurnAt =
      typeof entry.last_turn === 'string' ? Date.parse(entry.last_turn) : Number.NaN;
    const state: ConnectorState = failed
      ? 'failed'
      : entry.off === true
        ? 'off'
        : entry.scanning === true
          ? 'scanning'
          : 'collecting';
    connectors.push({
      name,
      state,
      ...(reason === '' ? {} : { reason }),
      turns: isCount(entry.turns) ? entry.turns : 0,
      skipped: isCount(entry.skipped) ? entry.skipped : 0,
      unreadable: isCount(entry.unreadable) ? entry.unreadable : 0,
      ...(Number.isNaN(lastTurnAt) ? {} : { lastTurnAt }),
    });
  }
  return connectors;
}

function isScreenState(value: unknown): value is ScreenState {
  return screenStates.some((state) => state === value);
}

/**
 * The `screen` row: its state, the samples of today and the time of the last. A row the window
 * does not know (another shape, another state) or one of a computer that cannot collect is not
 * shown; a field of another type is left out, not an error.
 */
export function parseScreen(value: unknown): ScreenCollection | undefined {
  if (!isRecord(value) || !isScreenState(value.state)) {
    return undefined;
  }
  const reason = typeof value.reason === 'string' ? value.reason.trim() : '';
  if (value.state === 'off' && reason === unsupportedReason) {
    return undefined;
  }
  const lastSampleAt =
    typeof value.last_sample === 'string' ? Date.parse(value.last_sample) : Number.NaN;
  return {
    state: value.state,
    ...(reason === '' || value.state === 'collecting' ? {} : { reason }),
    samplesToday: isCount(value.samples_today) ? value.samples_today : 0,
    ...(Number.isNaN(lastSampleAt) ? {} : { lastSampleAt }),
    missing: value.state === 'needs_permission' ? missingGrants(parseGrants(value)) : [],
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
