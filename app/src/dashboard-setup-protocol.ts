import { isCount, parseLine, replyError } from './agent-json';

/** Where a setup run is, as the agent names it. `idle`: no run has been started yet. */
export type SetupState =
  | 'idle'
  | 'writing_files'
  | 'reading_and_building'
  | 'done'
  | 'failed'
  | 'cancelled';

export const setupStates: readonly SetupState[] = [
  'idle',
  'writing_files',
  'reading_and_building',
  'done',
  'failed',
  'cancelled',
];

/** What the agent says of the run: counts and states, and the report once it is done. */
export interface SetupStatus {
  readonly state: SetupState;
  /** The session files written so far, and the bytes of text in them. */
  readonly sessions: number;
  readonly textBytes: number;
  /** The bytes sent to the model and received from it so far. */
  readonly sent: number;
  readonly received: number;
  /** A plain reason, when the run failed. */
  readonly error?: string;
  /** What the run found, in Markdown, when it is done. */
  readonly report?: string;
}

export type SetupResult =
  | { readonly ok: true; readonly status: SetupStatus }
  | { readonly ok: false; readonly error: string };

export interface DashboardSetupApi {
  readonly startDashboardSetup: () => Promise<SetupResult>;
  readonly getDashboardSetup: () => Promise<SetupResult>;
  readonly cancelDashboardSetup: () => Promise<SetupResult>;
}

export const SETUP_START_CHANNEL = 'dashboard-setup:start';
export const SETUP_STATUS_CHANNEL = 'dashboard-setup:status';
export const SETUP_CANCEL_CHANNEL = 'dashboard-setup:cancel';

/** The period a run reads, in days. The agent reads the same when it is not told. */
export const setupDays = 30;

/** The window asks for the period above and nothing else: it sends no argument of its own. */
export const setupStartLine = `dashboard-setup-start ${JSON.stringify({ days: setupDays })}`;
export const setupStatusLine = 'dashboard-setup-status';
export const setupCancelLine = 'dashboard-setup-cancel';

function isSetupState(value: unknown): value is SetupState {
  return setupStates.some((state) => state === value);
}

/** Turns a reply of one of the three setup commands into a result; anything else is an error. */
export function parseSetupReply(line: string): SetupResult {
  const reply = parseLine(line);
  // The status of a failed run has an `error` of its own, so only a reply with no state is a refusal.
  const error = 'state' in reply ? null : replyError(reply);
  if (error !== null) {
    return { ok: false, error };
  }
  const { state, sessions, text_bytes: textBytes, sent, received } = reply;
  if (
    !isSetupState(state) ||
    !isCount(sessions) ||
    !isCount(textBytes) ||
    !isCount(sent) ||
    !isCount(received)
  ) {
    throw new Error('the agent reply is not the state of a setup run');
  }
  const status: SetupStatus = { state, sessions, textBytes, sent, received };
  return {
    ok: true,
    status: {
      ...status,
      ...(typeof reply.error === 'string' ? { error: reply.error } : {}),
      ...(typeof reply.report === 'string' ? { report: reply.report } : {}),
    },
  };
}

/** Whether a run goes now. */
export function isRunning(state: SetupState): boolean {
  return state === 'writing_files' || state === 'reading_and_building';
}
