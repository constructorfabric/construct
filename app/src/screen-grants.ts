import { isRecord } from './agent-json';

/**
 * The permissions the app knows how to explain and to send the person to, by the word the agent
 * uses for each. A grant with another name is not shown: the app has no sentence for it.
 */
export const grantNames = [
  'accessibility',
  'screen_recording',
  'automation',
  'input_monitoring',
] as const;

export type GrantName = (typeof grantNames)[number];

/** What the person gave, as the agent tells it; the browser can also be closed, so not asked. */
export const grantStates = ['granted', 'denied', 'not_asked', 'browser_not_running'] as const;

export type GrantState = (typeof grantStates)[number];

export interface Grant {
  readonly name: GrantName;
  readonly state: GrantState;
}

export function isGrantName(value: unknown): value is GrantName {
  return grantNames.some((name) => name === value);
}

function isGrantState(value: unknown): value is GrantState {
  return grantStates.some((state) => state === value);
}

/**
 * The grants of the `screen` row of a status reply. What the app does not know (another name, another
 * state, another shape) is skipped, not an error: a state it cannot name is better not shown.
 */
export function parseGrants(screen: unknown): readonly Grant[] | undefined {
  if (!isRecord(screen) || !Array.isArray(screen.grants)) {
    return undefined;
  }
  return screen.grants.flatMap((entry: unknown): Grant[] =>
    isRecord(entry) && isGrantName(entry.name) && isGrantState(entry.state)
      ? [{ name: entry.name, state: entry.state }]
      : [],
  );
}
