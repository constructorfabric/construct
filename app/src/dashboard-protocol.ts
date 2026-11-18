import { isRecord, parseLine, replyError } from './agent-json';

/** Whose time the page counts: the user's own, the unattended agents', or both. */
export type Whose = 'own' | 'unattended' | 'all';

export const whoseValues: readonly Whose[] = ['own', 'unattended', 'all'];

/** One day as the window counts it: its label and its start and end in UTC, whole minutes. */
export interface DayBounds {
  readonly label: string;
  readonly start: string;
  readonly end: string;
}

export interface DashboardInput {
  readonly days: readonly DayBounds[];
  readonly sessions: Whose;
}

/** The minutes of one day of the answer. */
export interface DayMinutes {
  readonly label: string;
  readonly minutes: number;
}

/** The numbers of a period. The answer holds no text of a message. */
export type DashboardResult =
  | {
      readonly ok: true;
      readonly totalMinutes: number;
      readonly sessionCount: number;
      readonly days: readonly DayMinutes[];
    }
  | {
      readonly ok: false;
      readonly error: string;
      /** The agent is still reading the history: the numbers will come, and the page asks again. */
      readonly reading?: true;
    };

export interface DashboardApi {
  readonly getDashboardNumbers: (input: DashboardInput) => Promise<DashboardResult>;
}

/** What the agent answers while it builds the activity time from the history. */
export const readingHistoryError = 'the activity time is being built';

export const DASHBOARD_NUMBERS_CHANNEL = 'dashboard:numbers';

/** The most days one call may ask for: the agent's own limit. */
export const maxDays = 62;
const maxLabelChars = 32;
const wholeMinuteUtc = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:00)?Z$/;

export function isWhose(value: unknown): value is Whose {
  return whoseValues.some((whose) => whose === value);
}

function parseDay(value: unknown): DayBounds | null {
  if (!isRecord(value)) {
    return null;
  }
  const { label, start, end } = value;
  if (
    typeof label !== 'string' ||
    label === '' ||
    label.length > maxLabelChars ||
    typeof start !== 'string' ||
    typeof end !== 'string' ||
    !wholeMinuteUtc.test(start) ||
    !wholeMinuteUtc.test(end)
  ) {
    return null;
  }
  return { label, start, end };
}

/** The input of a bridge call, which comes from the window and so is checked. */
export function parseDashboardInput(value: unknown): DashboardInput | null {
  if (!isRecord(value) || !Array.isArray(value.days) || !isWhose(value.sessions)) {
    return null;
  }
  const raw: unknown[] = value.days;
  if (raw.length === 0 || raw.length > maxDays) {
    return null;
  }
  const days = raw.map(parseDay);
  if (days.some((day) => day === null)) {
    return null;
  }
  return { days: days as DayBounds[], sessions: value.sessions };
}

/** `dashboard-numbers {...}`: the days and whose sessions count; nothing else is sent. */
export function dashboardNumbersLine(input: DashboardInput): string {
  return `dashboard-numbers ${JSON.stringify({ days: input.days, sessions: input.sessions })}`;
}

function isNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

function parseDayMinutes(value: unknown): DayMinutes {
  if (!isRecord(value) || typeof value.label !== 'string' || !isNumber(value.minutes)) {
    throw new Error('the agent reply has a day that is not a label with minutes');
  }
  return { label: value.label, minutes: value.minutes };
}

/** Turns a `dashboard-numbers` reply into a result; the groups are not needed here. */
export function parseDashboardReply(line: string): DashboardResult {
  const reply = parseLine(line);
  const error = replyError(reply);
  if (error !== null) {
    return error === readingHistoryError
      ? { ok: false, error, reading: true }
      : { ok: false, error };
  }
  if (
    !isNumber(reply.total_minutes) ||
    !isNumber(reply.session_count) ||
    !Array.isArray(reply.days)
  ) {
    throw new Error('the agent reply has no numbers');
  }
  const days: unknown[] = reply.days;
  return {
    ok: true,
    totalMinutes: reply.total_minutes,
    sessionCount: reply.session_count,
    days: days.map(parseDayMinutes),
  };
}
