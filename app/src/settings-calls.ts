import {
  collectionSetLine,
  parseCollectionInput,
  parseModelInput,
  parseSettingsReply,
  parseTestLine,
  removeKeyLine,
  settingsGetLine,
  settingsSetLine,
  settingsTestLine,
} from './agent-settings';
import type { ModelInput, ModelTestResult, SettingsResult, TestProgress } from './agent-settings';
import {
  agentSocketPath,
  callAgent,
  failureText,
  settingsLimits,
  streamAgent,
} from './agent-socket';
import type { StreamLimits, Where } from './agent-socket';

const keyForAnotherAddress =
  'the saved key belongs to the saved address: type the key again to use another address';
const testAlreadyRunning = 'a Test is already running';
const invalidInput = 'the window sent settings in a form the app does not accept';

async function settingsCall(where: Where, line: string): Promise<SettingsResult> {
  try {
    return parseSettingsReply(await callAgent(where.env, where.platform, line, settingsLimits));
  } catch (error) {
    return { ok: false, error: failureText(error) };
  }
}

/**
 * The agent gives each check of a Test 15 s, and the checks run one after the other; the app
 * waits a little longer for the next line, so the agent's own answer wins.
 */
export const testStreamLimits: StreamLimits = {
  idleMs: 20_000,
  maxLineChars: 4096,
  maxTotalChars: 64 * 1024,
};

/**
 * One Test: each line the agent sends goes to `onProgress` while it is a check line, and the last
 * line is the result. A failed result carries no checks.
 */
function testCall(
  where: Where,
  line: string,
  onProgress: (progress: TestProgress) => void,
): Promise<ModelTestResult> {
  return new Promise((resolve) => {
    let path: string;
    try {
      path = agentSocketPath(where.env, where.platform);
    } catch (error) {
      resolve({ result: 'failed', error: failureText(error), checks: [] });
      return;
    }
    let result: ModelTestResult | null = null;
    const stream = streamAgent(
      path,
      line,
      testStreamLimits,
      (reply) => {
        if (result !== null) {
          return;
        }
        try {
          const parsed = parseTestLine(reply);
          if (parsed.kind === 'progress') {
            onProgress(parsed.progress);
          } else {
            result = parsed.result;
          }
        } catch (error) {
          stream.close();
          resolve({ result: 'failed', error: failureText(error), checks: [] });
        }
      },
      (error) => {
        if (result !== null) {
          resolve(result);
        } else {
          resolve({
            result: 'failed',
            error:
              error === null ? 'the agent closed the socket without an answer' : failureText(error),
            checks: [],
          });
        }
      },
    );
  });
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
export function createSettingsHandlers(
  where: Where,
  onTestProgress: (progress: TestProgress) => void = () => undefined,
): SettingsHandlers {
  // Progress events carry no run id, so only one Test runs at a time.
  let testing = false;
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
        return { result: 'failed', error: invalidInput, checks: [] };
      }
      if (await sendsSavedKeyElsewhere(where, input)) {
        return { result: 'failed', error: keyForAnotherAddress, checks: [] };
      }
      if (testing) {
        return { result: 'failed', error: testAlreadyRunning, checks: [] };
      }
      testing = true;
      try {
        return await testCall(where, settingsTestLine(input), onTestProgress);
      } finally {
        testing = false;
      }
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
