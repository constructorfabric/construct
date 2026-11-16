import {
  collectionSetLine,
  parseCollectionInput,
  parseModelInput,
  parseSettingsReply,
  parseTestReply,
  removeKeyLine,
  settingsGetLine,
  settingsSetLine,
  settingsTestLine,
} from './agent-settings';
import type { ModelInput, ModelTestResult, SettingsResult } from './agent-settings';
import { callAgent, failureText, settingsLimits, testLimits } from './agent-socket';
import type { CallLimits, Where } from './agent-socket';

const keyForAnotherAddress =
  'the saved key belongs to the saved address: type the key again to use another address';
const invalidInput = 'the window sent settings in a form the app does not accept';

async function settingsCall(where: Where, line: string): Promise<SettingsResult> {
  try {
    return parseSettingsReply(await callAgent(where.env, where.platform, line, settingsLimits));
  } catch (error) {
    return { ok: false, error: failureText(error) };
  }
}

async function testCall(where: Where, line: string, limits: CallLimits): Promise<ModelTestResult> {
  try {
    return parseTestReply(await callAgent(where.env, where.platform, line, limits));
  } catch (error) {
    return { result: 'failed', error: failureText(error) };
  }
}

/**
 * Whether `input` would send the saved key to an address other than the saved one: no key typed,
 * a key saved, and an address that differs. The window is not trusted, so a script in it must not
 * be able to walk the saved key to an address of its choice; a new address needs the key typed.
 */
async function sendsSavedKeyElsewhere(where: Where, input: ModelInput): Promise<boolean> {
  if ((input.apiKey ?? '') !== '' || input.baseUrl === '') {
    return false;
  }
  const saved = await settingsCall(where, settingsGetLine);
  // When the saved settings cannot be read the call is refused too: it cannot be shown safe.
  return !saved.ok || (saved.model.apiKeySet && saved.model.baseUrl !== input.baseUrl);
}

export interface SettingsHandlers {
  readonly get: () => Promise<SettingsResult>;
  readonly save: (value: unknown) => Promise<SettingsResult>;
  readonly test: (value: unknown) => Promise<ModelTestResult>;
  readonly removeKey: () => Promise<SettingsResult>;
  readonly saveCollection: (value: unknown) => Promise<SettingsResult>;
}

/**
 * What the five settings channels do. The window's input is checked here, since the window is
 * not trusted, and nothing in these handlers logs it: a handler that throws would put its
 * message in the console, so each one answers failures as values.
 */
export function createSettingsHandlers(where: Where): SettingsHandlers {
  return {
    get: () => settingsCall(where, settingsGetLine),
    save: async (value) => {
      const input = parseModelInput(value);
      if (input === null) {
        return { ok: false, error: invalidInput };
      }
      if (await sendsSavedKeyElsewhere(where, input)) {
        return { ok: false, error: keyForAnotherAddress };
      }
      return settingsCall(where, settingsSetLine(input));
    },
    test: async (value) => {
      const input = parseModelInput(value);
      if (input === null) {
        return { result: 'failed', error: invalidInput };
      }
      if (await sendsSavedKeyElsewhere(where, input)) {
        return { result: 'failed', error: keyForAnotherAddress };
      }
      return testCall(where, settingsTestLine(input), testLimits);
    },
    removeKey: () => settingsCall(where, removeKeyLine),
    saveCollection: async (value) => {
      const input = parseCollectionInput(value);
      if (input === null) {
        return { ok: false, error: invalidInput };
      }
      return settingsCall(where, collectionSetLine(input));
    },
  };
}
