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

/** What a widget keeps: a list left out means no limit on it. */
export interface Filter {
  readonly projects?: readonly string[];
  readonly tickets?: readonly string[];
  readonly tools?: readonly string[];
}

/**
 * What to count in the days: a widget of the dashboard document, which has its own sessions,
 * filter and grouping, or the sessions and filter the call names.
 */
export type DashboardInput =
  | { readonly days: readonly DayBounds[]; readonly widget: string }
  | {
      readonly days: readonly DayBounds[];
      readonly sessions: Whose;
      readonly filter?: Filter;
    };

/** The minutes of one day of the answer. */
export interface DayMinutes {
  readonly label: string;
  readonly minutes: number;
}

/** One group of a table: its key, its time, its share of the total (0 to 1) and its sessions. */
export interface GroupMinutes {
  readonly key: string;
  readonly minutes: number;
  readonly share: number;
  readonly sessions: number;
}

/** The numbers of a period. The answer holds no text of a message. */
export type DashboardResult =
  | {
      readonly ok: true;
      readonly totalMinutes: number;
      readonly sessionCount: number;
      readonly days: readonly DayMinutes[];
      readonly groups: readonly GroupMinutes[];
      /** How many hidden projects have time in the days: what the totals leave out. */
      readonly hiddenProjects: number;
    }
  | {
      readonly ok: false;
      readonly error: string;
      /** The agent is still reading the history: the numbers will come, and the page asks again. */
      readonly reading?: true;
    };

/** What a widget draws. */
export type WidgetKind = 'total' | 'bars_per_day' | 'table' | 'day_summary';

/** What a table groups the time by. */
export type GroupBy = 'project' | 'ticket' | 'tool' | 'collection' | 'day';

/** The period a widget covers. */
export type Period =
  | 'today'
  | 'this_week'
  | 'last_week'
  | 'this_month'
  | 'last_month'
  | 'last_7_days'
  | 'last_30_days';

/** Where a widget sits on the grid of 12 columns; columns and rows count from 0. */
export interface Position {
  readonly column: number;
  readonly row: number;
  readonly width: number;
  readonly height: number;
}

export interface Widget {
  readonly id: string;
  readonly kind: WidgetKind;
  readonly groupBy?: GroupBy;
  readonly period?: Period;
  readonly sessions?: Whose;
  readonly filter?: Filter;
  readonly position: Position;
}

/** The part of the dashboard document the page draws. */
export interface DashboardDocument {
  readonly title: string;
  /** The period and the sessions of every widget that has none of its own. */
  readonly period: Period;
  readonly sessions: Whose;
  readonly widgets: readonly Widget[];
  /** The projects the page leaves out, as the document names them. */
  readonly hiddenProjects: readonly string[];
  /** The name the page shows for a project, by the name the document gives the project. */
  readonly projectNames: Readonly<Record<string, string>>;
}

/** Why the file as it is now is not used, and where: a line and column or a JSON pointer. */
export interface DocumentProblem {
  readonly kind: 'file' | 'line' | 'pointer';
  readonly place: string;
  readonly message: string;
}

/** The last good document, if there was one, and the problem of the file as it is now. */
export type DocumentResult =
  | {
      readonly ok: true;
      readonly document: DashboardDocument | null;
      readonly problem: DocumentProblem | null;
    }
  | { readonly ok: false; readonly error: string };

/** A change of the projects the page shows: hide one, show one again, or show one under a name. */
export type ProjectChange =
  | { readonly action: 'hide' | 'show'; readonly project: string }
  | { readonly action: 'rename'; readonly project: string; readonly name: string };

export interface DashboardApi {
  readonly getDashboardNumbers: (input: DashboardInput) => Promise<DashboardResult>;
  readonly getDashboardDocument: () => Promise<DocumentResult>;
  /** Writes the change into the dashboard document; the answer is the document as it is then. */
  readonly changeDashboardProject: (change: ProjectChange) => Promise<DocumentResult>;
}

/** What the agent answers while it builds the activity time from the history. */
export const readingHistoryError = 'the activity time is being built';

export const DASHBOARD_NUMBERS_CHANNEL = 'dashboard:numbers';
export const DASHBOARD_DOCUMENT_CHANNEL = 'dashboard:document';
export const DASHBOARD_PROJECT_CHANNEL = 'dashboard:project';

/** The most days one call may ask for: the agent's own limit. */
export const maxDays = 62;
const maxLabelChars = 32;
const wholeMinuteUtc = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:00)?Z$/;
const widgetId = /^[a-z0-9_-]{1,64}$/;
const maxFilterNames = 100;
export const maxNameChars = 512;

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

function parseNames(value: unknown): string[] | null {
  if (!Array.isArray(value) || value.length > maxFilterNames) {
    return null;
  }
  const names: unknown[] = value;
  return names.every((name) => typeof name === 'string' && name.length <= maxNameChars)
    ? (names as string[])
    : null;
}

/** A filter of the window; a list the filter does not have stays out of the result. */
function parseFilter(value: unknown): Filter | null {
  if (!isRecord(value)) {
    return null;
  }
  const filter: { projects?: string[]; tickets?: string[]; tools?: string[] } = {};
  for (const name of ['projects', 'tickets', 'tools'] as const) {
    if (value[name] !== undefined && value[name] !== null) {
      const names = parseNames(value[name]);
      if (names === null) {
        return null;
      }
      filter[name] = names;
    }
  }
  return filter;
}

function parseDays(value: unknown): DayBounds[] | null {
  if (!Array.isArray(value)) {
    return null;
  }
  const raw: unknown[] = value;
  if (raw.length === 0 || raw.length > maxDays) {
    return null;
  }
  const days = raw.map(parseDay);
  return days.some((day) => day === null) ? null : (days as DayBounds[]);
}

/** The input of a bridge call, which comes from the window and so is checked. */
export function parseDashboardInput(value: unknown): DashboardInput | null {
  if (!isRecord(value)) {
    return null;
  }
  const days = parseDays(value.days);
  if (days === null) {
    return null;
  }
  if (value.widget !== undefined) {
    // A widget carries its own counting: naming the sessions or a filter too is a mistake.
    if (
      typeof value.widget !== 'string' ||
      !widgetId.test(value.widget) ||
      value.sessions !== undefined ||
      value.filter !== undefined
    ) {
      return null;
    }
    return { days, widget: value.widget };
  }
  if (!isWhose(value.sessions)) {
    return null;
  }
  if (value.filter === undefined) {
    return { days, sessions: value.sessions };
  }
  const filter = parseFilter(value.filter);
  return filter === null ? null : { days, sessions: value.sessions, filter };
}

/** `dashboard-numbers {...}`: the days and the widget, or the sessions and filter; nothing else. */
export function dashboardNumbersLine(input: DashboardInput): string {
  const request =
    'widget' in input
      ? { days: input.days, widget: input.widget }
      : { days: input.days, sessions: input.sessions, filter: input.filter };
  return `dashboard-numbers ${JSON.stringify(request)}`;
}

export const dashboardGetLine = 'dashboard-get';

const actions = ['hide', 'show', 'rename'] as const;

function isProjectText(value: unknown): value is string {
  return typeof value === 'string' && value.trim() !== '' && value.length <= maxNameChars;
}

/** The change of a bridge call, which comes from the window and so is checked. */
export function parseProjectChange(value: unknown): ProjectChange | null {
  if (!isRecord(value)) {
    return null;
  }
  const { action, project, name } = value;
  const known = actions.find((candidate) => candidate === action);
  if (known === undefined || !isProjectText(project)) {
    return null;
  }
  if (known === 'rename') {
    return isProjectText(name) && Object.keys(value).length === 3
      ? { action: known, project, name: name.trim() }
      : null;
  }
  return Object.keys(value).length === 2 ? { action: known, project } : null;
}

/** `dashboard-hide {...}`, `dashboard-show {...}` or `dashboard-rename {...}`. */
export function projectChangeLine(change: ProjectChange): string {
  const request =
    change.action === 'rename'
      ? { project: change.project, name: change.name }
      : { project: change.project };
  return `dashboard-${change.action} ${JSON.stringify(request)}`;
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

function parseGroup(value: unknown): GroupMinutes {
  if (
    !isRecord(value) ||
    typeof value.key !== 'string' ||
    !isNumber(value.minutes) ||
    !isNumber(value.share) ||
    !isNumber(value.session_count)
  ) {
    throw new Error('the agent reply has a group that is not a key with minutes');
  }
  return {
    key: value.key,
    minutes: value.minutes,
    share: value.share,
    sessions: value.session_count,
  };
}

/** Turns a `dashboard-numbers` reply into a result. */
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
    !Array.isArray(reply.days) ||
    !Array.isArray(reply.groups) ||
    !isNumber(reply.hidden_project_count)
  ) {
    throw new Error('the agent reply has no numbers');
  }
  const days: unknown[] = reply.days;
  const groups: unknown[] = reply.groups;
  return {
    ok: true,
    totalMinutes: reply.total_minutes,
    sessionCount: reply.session_count,
    days: days.map(parseDayMinutes),
    groups: groups.map(parseGroup),
    hiddenProjects: reply.hidden_project_count,
  };
}

const periods: readonly Period[] = [
  'today',
  'this_week',
  'last_week',
  'this_month',
  'last_month',
  'last_7_days',
  'last_30_days',
];
const kinds: readonly WidgetKind[] = ['total', 'bars_per_day', 'table', 'day_summary'];
const groupings: readonly GroupBy[] = ['project', 'ticket', 'tool', 'collection', 'day'];

function oneOf<T extends string>(values: readonly T[], value: unknown, what: string): T {
  const found = values.find((candidate) => candidate === value);
  if (found === undefined) {
    throw new Error(`the agent reply has ${what} the app does not know`);
  }
  return found;
}

function parseNumber(value: unknown, what: string): number {
  if (typeof value !== 'number' || !Number.isInteger(value)) {
    throw new Error(`the agent reply has ${what} that is not a whole number`);
  }
  return value;
}

function parseWidget(value: unknown): Widget {
  if (!isRecord(value) || typeof value.id !== 'string' || !isRecord(value.position)) {
    throw new Error('the agent reply has a widget that is not an id with a position');
  }
  const at = value.position;
  // The agent's checker lets a name be `null`, as it lets one be left out.
  const filter = value.filter == null ? undefined : parseFilter(value.filter);
  if (filter === null) {
    throw new Error('the agent reply has a filter that is not lists of names');
  }
  return {
    id: value.id,
    kind: oneOf(kinds, value.kind, 'a kind of widget'),
    ...(value.group_by == null ? {} : { groupBy: oneOf(groupings, value.group_by, 'a grouping') }),
    ...(value.period == null ? {} : { period: oneOf(periods, value.period, 'a period') }),
    ...(value.sessions == null
      ? {}
      : { sessions: oneOf(whoseValues, value.sessions, 'a choice of sessions') }),
    ...(filter === undefined ? {} : { filter }),
    position: {
      column: parseNumber(at.column, 'a column'),
      row: parseNumber(at.row, 'a row'),
      width: parseNumber(at.width, 'a width'),
      height: parseNumber(at.height, 'a height'),
    },
  };
}

function parseDocument(value: unknown): DashboardDocument {
  if (!isRecord(value) || typeof value.title !== 'string' || !isRecord(value.payload)) {
    throw new Error('the agent reply has a document that is not a title with a payload');
  }
  const { payload } = value;
  if (!Array.isArray(payload.widgets)) {
    throw new Error('the agent reply has a document with no widgets');
  }
  const widgets: unknown[] = payload.widgets;
  const parsed = {
    title: value.title,
    period: oneOf(periods, payload.period, 'a period'),
    sessions: oneOf(whoseValues, payload.sessions, 'a choice of sessions'),
    widgets: widgets.map(parseWidget),
  };
  const hidden: unknown = payload.hidden_projects;
  const names: unknown = payload.project_names;
  if (!Array.isArray(hidden) || !hidden.every((project) => typeof project === 'string')) {
    throw new Error('the agent reply has hidden projects that are not a list of names');
  }
  if (!isRecord(names) || !Object.values(names).every((name) => typeof name === 'string')) {
    throw new Error('the agent reply has project names that are not names');
  }
  return {
    ...parsed,
    hiddenProjects: hidden as string[],
    projectNames: names as Record<string, string>,
  };
}

function parseProblem(value: unknown): DocumentProblem {
  if (!isRecord(value) || typeof value.place !== 'string' || typeof value.message !== 'string') {
    throw new Error('the agent reply has a problem that is not a place with a message');
  }
  const kind = oneOf(['file', 'line', 'pointer'] as const, value.kind, 'a kind of problem');
  return { kind, place: value.place, message: value.message };
}

/**
 * Turns a `dashboard-get` reply into a result: the last good document (none when there was none)
 * and the problem of the file as it is now (none when the file passes).
 */
export function parseDocumentReply(line: string): DocumentResult {
  const reply = parseLine(line);
  const error = replyError(reply);
  if (error !== null) {
    return { ok: false, error };
  }
  if (reply.document === undefined) {
    throw new Error('the agent reply has no document');
  }
  return {
    ok: true,
    document: reply.document === null ? null : parseDocument(reply.document),
    problem: reply.error === undefined ? null : parseProblem(reply.error),
  };
}
