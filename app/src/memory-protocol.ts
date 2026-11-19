import { isRecord, parseLine, replyError } from './agent-json';

/** A JSON value, as a fact's value may hold it. */
export type JsonValue =
  | string
  | number
  | boolean
  | null
  | readonly JsonValue[]
  | { readonly [key: string]: JsonValue };

/** The categories of a person's profile, as the gear names them. */
export const entityKinds = ['identity', 'role', 'skill', 'preference'] as const;
export type EntityKind = (typeof entityKinds)[number];

/**
 * One fact of the person, with the gear's own names. Only what the Memory page shows is kept: who
 * a fact came from and its review state are not shown yet.
 */
export interface MemoryFact {
  readonly fact_id: string;
  readonly entity_kind: EntityKind;
  /** The property by its plain key, such as `skills`. */
  readonly property: string;
  /** A string, or an object for a property with fields of its own. */
  readonly value: JsonValue;
  /** When the fact was stored, as the gear writes it (RFC 3339). */
  readonly stored_at: string;
}

export type FactsResult =
  | {
      readonly ok: true;
      readonly facts: readonly MemoryFact[];
      /** The profile's version; `null` for a person without a profile. */
      readonly version: number | null;
    }
  | { readonly ok: false; readonly error: string };

export type VersionResult =
  | { readonly ok: true; readonly version: number | null }
  | { readonly ok: false; readonly error: string };

/** The line that has the agent erase the person; it takes no argument. */
export const eraseLine = 'memory-erase';

/**
 * Turns a `memory-erase` reply into a result. `started` and `running` are both an erasure that
 * the gear carries on; an error holds the agent's own fixed reason.
 */
export function parseEraseReply(line: string): EraseResult {
  const reply = parseLine(line);
  const error = replyError(reply);
  if (error !== null) {
    return { ok: false, error };
  }
  if (reply.erasure !== 'started' && reply.erasure !== 'running') {
    throw new Error('the agent reply says nothing of an erasure');
  }
  return { ok: true, running: reply.erasure === 'running' };
}

/** The gear's settings of the person, as far as the page needs them. */
export function parseErasing(body: unknown): boolean | null {
  return isRecord(body) && typeof body.erasure_in_progress === 'boolean'
    ? body.erasure_in_progress
    : null;
}

const factIdShape = /^[\w.~:-]{1,128}$/;

/**
 * Whether an id may go into a route: a short text of letters, digits and a few marks, and not a
 * dot segment. The page offers Delete only for such an id.
 */
export function isDeletableFactId(factId: string): boolean {
  return factIdShape.test(factId) && factId !== '.' && factId !== '..';
}

/** Whether a fact is gone: it was deleted, or it was already not there. */
export type FactDeleteResult =
  | { readonly ok: true }
  | { readonly ok: false; readonly error: string };

/**
 * The answer to "delete everything": the agent switched collection off and the gear has started
 * to erase, or was already erasing. Either way the page then waits for the gear to finish.
 */
export type EraseResult =
  /** `running`: an erasure was already on, and this request changed nothing. */
  { readonly ok: true; readonly running: boolean } | { readonly ok: false; readonly error: string };

/** Whether the gear is still erasing the person, read from its settings route. */
export type ErasingResult =
  | { readonly ok: true; readonly erasing: boolean }
  | { readonly ok: false; readonly error: string };

export interface MemoryApi {
  /** Every fact of the person, over all pages, with the profile's version at the first page. */
  readonly getMemoryFacts: () => Promise<FactsResult>;
  /** Only the profile's version, from a page of one fact. */
  readonly getMemoryVersion: () => Promise<VersionResult>;
  /** Deletes one fact by its id. Sending it again after a failure is safe. */
  readonly deleteMemoryFact: (factId: string) => Promise<FactDeleteResult>;
  /** Asks the agent to erase everything about the person; it does not wait for the end. */
  readonly eraseEverything: () => Promise<EraseResult>;
  /** Whether the gear still reports an erasure that runs. */
  readonly getErasing: () => Promise<ErasingResult>;
}

export const MEMORY_FACTS_CHANNEL = 'memory:facts';
export const MEMORY_VERSION_CHANNEL = 'memory:version';
export const MEMORY_DELETE_CHANNEL = 'memory:delete';
export const MEMORY_ERASE_CHANNEL = 'memory:erase';
export const MEMORY_ERASING_CHANNEL = 'memory:erasing';

/** What the app says when it cannot reach the gear; fixed texts that never hold an answer. */
export const reasonAgentNotRunning = 'the agent is not running';
export const reasonNotReady = 'Construct is not ready';

/**
 * Deepest nesting the page draws. What lies deeper is kept as its JSON text, so one fact with a
 * deep value is still listed with the rest.
 */
const maxDepth = 8;

function limitDepth(value: unknown, depth: number): JsonValue {
  if (value === null || ['string', 'number', 'boolean'].includes(typeof value)) {
    return value as JsonValue;
  }
  if (depth >= maxDepth) {
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) {
    return value.map((item) => limitDepth(item, depth + 1));
  }
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>).map(([key, item]) => [
      key,
      limitDepth(item, depth + 1),
    ]),
  );
}

function isEntityKind(value: unknown): value is EntityKind {
  return entityKinds.some((kind) => kind === value);
}

function parseFact(value: unknown): MemoryFact | null {
  if (!isRecord(value)) {
    return null;
  }
  const { fact_id, entity_kind, property, value: factValue, stored_at } = value;
  if (
    typeof fact_id !== 'string' ||
    !isEntityKind(entity_kind) ||
    typeof property !== 'string' ||
    property === '' ||
    typeof stored_at !== 'string' ||
    Number.isNaN(Date.parse(stored_at)) ||
    // The gear stores a string or an object; nothing else is a fact the page can draw.
    !(typeof factValue === 'string' || (isRecord(factValue) && !Array.isArray(factValue)))
  ) {
    return null;
  }
  return { fact_id, entity_kind, property, value: limitDepth(factValue, 0), stored_at };
}

/** One page of the facts list. */
export interface FactsPage {
  readonly facts: readonly MemoryFact[];
  readonly nextCursor: string | null;
  readonly version: number | null;
}

const maxCursorChars = 4096;

/** The gear's page, or `null` when its answer is not of the shape the gear documents. */
export function parseFactsPage(body: unknown): FactsPage | null {
  if (!isRecord(body) || !Array.isArray(body.items) || !isRecord(body.page_info)) {
    return null;
  }
  const { next_cursor: cursor } = body.page_info;
  if (
    cursor !== undefined &&
    cursor !== null &&
    (typeof cursor !== 'string' || cursor === '' || cursor.length > maxCursorChars)
  ) {
    return null;
  }
  const { version } = body;
  if (
    version !== undefined &&
    version !== null &&
    !(typeof version === 'number' && Number.isSafeInteger(version) && version >= 0)
  ) {
    return null;
  }
  const facts: MemoryFact[] = [];
  for (const item of body.items) {
    const fact = parseFact(item);
    if (fact === null) {
      return null;
    }
    facts.push(fact);
  }
  return { facts, nextCursor: cursor ?? null, version: version ?? null };
}
