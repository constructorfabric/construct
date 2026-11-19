import { isRecord, parseLine, replyError } from './agent-json';
/** What the window sees of the model settings. The API key is never part of it. */
export interface ModelSettings {
  readonly baseUrl: string;
  readonly model: string;
  /** Whether the agent holds a key. The key itself never comes back. */
  readonly apiKeySet: boolean;
}

/** The fields of the form. `apiKey` is left out when the user did not type one. */
export interface ModelInput {
  readonly baseUrl: string;
  readonly model: string;
  readonly apiKey?: string;
}

/** What the agent collects: all of it, each tool on its own, and the apps and websites. */
export interface CollectionSettings {
  readonly enabled: boolean;
  readonly claudeCode: boolean;
  readonly codex: boolean;
  /** The time in the person's apps and websites: off until the person turns it on. */
  readonly screen: boolean;
}

/** The switches a call changes; a switch left out keeps its value. */
export type CollectionInput = Partial<CollectionSettings>;

/** The outcome of a settings call: the settings now in force, or a text to show. */
export type SettingsResult =
  | {
      readonly ok: true;
      readonly model: ModelSettings;
      readonly collection: CollectionSettings;
      /** Whether this computer can collect the time in apps and websites. */
      readonly supportsScreen: boolean;
      /** Why collection is off without the user's say, when the agent says so. */
      readonly notice: string | null;
    }
  | { readonly ok: false; readonly error: string };

/** The results of a check or of a Test that carry nothing but their name. */
export const plainFailures = [
  'key refused',
  'model not found',
  'unreachable',
  'timeout',
  'tools refused',
  'no tool call',
  'bad tool call',
  'wrong answer',
] as const;

export type PlainFailure = (typeof plainFailures)[number];

function isPlainFailure(result: unknown): result is PlainFailure {
  return plainFailures.some((failure) => failure === result);
}

/** How a model Test ended. `failed` is a call that did not reach the model at all. */
export type TestOutcome =
  | { readonly result: 'works'; readonly text: string }
  | { readonly result: PlainFailure }
  | { readonly result: 'error'; readonly status: number }
  | { readonly result: 'failed'; readonly error: string };

/** How one check ended: a check that did not run is `skipped`. */
export type CheckResult =
  | { readonly result: 'works' }
  | { readonly result: 'skipped' }
  | { readonly result: PlainFailure }
  | { readonly result: 'error'; readonly status: number };

/** One named check of the Test, as the agent names it (`answers`, `uses tools`). */
export type TestCheck = CheckResult & { readonly name: string };

/** The result of a Test and, when the agent sends them, its checks (none from an older agent). */
export type ModelTestResult = TestOutcome & { readonly checks: readonly TestCheck[] };

/** What the window hears while a Test runs: the names of the checks, then each check as it ends. */
export type TestProgress =
  | { readonly event: 'checks'; readonly names: readonly string[] }
  | { readonly event: 'check'; readonly check: TestCheck };

export interface ModelSettingsApi {
  readonly getSettings: () => Promise<SettingsResult>;
  readonly saveModel: (input: ModelInput) => Promise<SettingsResult>;
  readonly testModel: (input: ModelInput) => Promise<ModelTestResult>;
  /** Calls `listener` with the progress of each Test; returns the way to stop listening. */
  readonly onTestProgress: (listener: (progress: TestProgress) => void) => () => void;
  readonly removeKey: () => Promise<SettingsResult>;
  readonly saveCollection: (input: CollectionInput) => Promise<SettingsResult>;
}

export const SETTINGS_GET_CHANNEL = 'settings:get';
export const SETTINGS_SAVE_CHANNEL = 'settings:save-model';
export const SETTINGS_TEST_CHANNEL = 'settings:test-model';
export const SETTINGS_TEST_PROGRESS_CHANNEL = 'settings:test-progress';
export const SETTINGS_REMOVE_KEY_CHANNEL = 'settings:remove-key';
export const SETTINGS_COLLECTION_CHANNEL = 'settings:save-collection';

export const DEFAULT_BASE_URL = 'https://api.openai.com/v1';
export const DEFAULT_MODEL = 'gpt-6-luna';

/** The settings of a page after a collection reply: only its collection and notice are taken. */
export function withCollectionReply(
  current: SettingsResult | null,
  reply: { readonly collection: CollectionSettings; readonly notice: string | null },
): SettingsResult | null {
  return current?.ok ? { ...current, collection: reply.collection, notice: reply.notice } : current;
}

/** The input of a bridge call, which comes from the window and so is checked. */
export function parseModelInput(value: unknown): ModelInput | null {
  if (!isRecord(value)) {
    return null;
  }
  const { baseUrl, model, apiKey } = value;
  if (typeof baseUrl !== 'string' || typeof model !== 'string') {
    return null;
  }
  if (apiKey === undefined) {
    return { baseUrl, model };
  }
  return typeof apiKey === 'string' ? { baseUrl, model, apiKey } : null;
}

/** The input of a collection call: the switches it names, each a boolean, at least one. */
export function parseCollectionInput(value: unknown): CollectionInput | null {
  if (!isRecord(value)) {
    return null;
  }
  const { enabled, claudeCode, codex, screen } = value;
  const given = [enabled, claudeCode, codex, screen];
  if (given.every((flag) => flag === undefined)) {
    return null;
  }
  if (given.some((flag) => flag !== undefined && typeof flag !== 'boolean')) {
    return null;
  }
  return {
    ...(typeof enabled === 'boolean' && { enabled }),
    ...(typeof claudeCode === 'boolean' && { claudeCode }),
    ...(typeof codex === 'boolean' && { codex }),
    ...(typeof screen === 'boolean' && { screen }),
  };
}

/** `settings-set {"collection":{...}}`: the agent's names, and only the switches given. */
export function collectionSetLine(input: CollectionInput): string {
  const collection = {
    enabled: input.enabled,
    claude_code: input.claudeCode,
    codex: input.codex,
    screen: input.screen,
  };
  return `settings-set ${JSON.stringify({ collection })}`;
}

/** `{"model":{...}}` for `settings-set`; the key is sent only when there is one. */
export function settingsSetLine(input: ModelInput): string {
  const model: Record<string, string> = { base_url: input.baseUrl, model: input.model };
  if (input.apiKey !== undefined && input.apiKey !== '') {
    model.api_key = input.apiKey;
  }
  return `settings-set ${JSON.stringify({ model })}`;
}

export const settingsGetLine = 'settings-get';
export const removeKeyLine = `settings-set ${JSON.stringify({ model: { remove_key: true } })}`;

/**
 * `settings-test {...}`: the typed values; a missing one is taken from the saved settings. The
 * agent answers a line for each check before the last line.
 */
export function settingsTestLine(input: ModelInput): string {
  const body: Record<string, string | boolean> = {};
  if (input.baseUrl !== '') {
    body.base_url = input.baseUrl;
  }
  if (input.model !== '') {
    body.model = input.model;
  }
  if (input.apiKey !== undefined && input.apiKey !== '') {
    body.api_key = input.apiKey;
  }
  body.progress = true;
  return `settings-test ${JSON.stringify(body)}`;
}

/** Turns a `settings-get` or `settings-set` reply into a result. */
export function parseSettingsReply(line: string): SettingsResult {
  const reply = parseLine(line);
  const error = replyError(reply);
  if (error !== null) {
    return { ok: false, error };
  }
  const { model } = reply;
  if (
    !isRecord(model) ||
    typeof model.base_url !== 'string' ||
    typeof model.model !== 'string' ||
    typeof model.api_key_set !== 'boolean'
  ) {
    throw new Error('the agent reply has no model settings');
  }
  const { collection, notice } = reply;
  if (
    !isRecord(collection) ||
    typeof collection.enabled !== 'boolean' ||
    typeof collection.claude_code !== 'boolean' ||
    typeof collection.codex !== 'boolean'
  ) {
    throw new Error('the agent reply has no collection settings');
  }
  const { supports } = reply;
  if (
    typeof collection.screen !== 'boolean' ||
    !isRecord(supports) ||
    typeof supports.screen !== 'boolean'
  ) {
    throw new Error('the agent reply has no screen settings');
  }
  return {
    ok: true,
    model: { baseUrl: model.base_url, model: model.model, apiKeySet: model.api_key_set },
    collection: {
      enabled: collection.enabled,
      claudeCode: collection.claude_code,
      codex: collection.codex,
      screen: collection.screen,
    },
    supportsScreen: supports.screen,
    notice: typeof notice === 'string' ? notice : null,
  };
}

function parseCheck(value: unknown): TestCheck {
  if (!isRecord(value) || typeof value.name !== 'string' || typeof value.result !== 'string') {
    throw new Error('the agent reply has a check that is not readable');
  }
  const { name, result } = value;
  if (result === 'works' || result === 'skipped') {
    return { name, result };
  }
  if (result === 'error') {
    if (typeof value.status !== 'number') {
      throw new Error('the agent reply has no status');
    }
    return { name, result, status: value.status };
  }
  if (isPlainFailure(result)) {
    return { name, result };
  }
  throw new Error('the agent gave an unknown check result');
}

function parseChecks(value: unknown): readonly TestCheck[] {
  if (value === undefined) {
    return [];
  }
  if (!Array.isArray(value)) {
    throw new Error('the agent reply has checks that are not a list');
  }
  return value.map(parseCheck);
}

function parseOutcome(reply: Record<string, unknown>): TestOutcome {
  const error = replyError(reply);
  if (error !== null) {
    return { result: 'failed', error };
  }
  switch (reply.result) {
    case 'works':
      if (typeof reply.text !== 'string') {
        throw new Error('the agent reply has no text');
      }
      return { result: 'works', text: reply.text };
    case 'error':
      if (typeof reply.status !== 'number') {
        throw new Error('the agent reply has no status');
      }
      return { result: 'error', status: reply.status };
    default:
      if (isPlainFailure(reply.result)) {
        return { result: reply.result };
      }
      throw new Error('the agent gave an unknown test result');
  }
}

/** One line of a `settings-test` answer: the names, a check as it ends, or the last line. */
export type TestLine =
  | { readonly kind: 'progress'; readonly progress: TestProgress }
  | { readonly kind: 'last'; readonly result: ModelTestResult };

/**
 * Turns a line of a `settings-test` answer into what it is. The last line has the result and,
 * from an agent that sends them, the entry of every check; the provider's own text is never in it.
 */
export function parseTestLine(line: string): TestLine {
  const reply = parseLine(line);
  if (Array.isArray(reply.checks) && Object.keys(reply).length === 1) {
    if (!reply.checks.every((name) => typeof name === 'string')) {
      throw new Error('the agent reply has a check name that is not text');
    }
    return { kind: 'progress', progress: { event: 'checks', names: reply.checks as string[] } };
  }
  if (reply.check !== undefined) {
    return { kind: 'progress', progress: { event: 'check', check: parseCheck(reply.check) } };
  }
  return { kind: 'last', result: { ...parseOutcome(reply), checks: parseChecks(reply.checks) } };
}
