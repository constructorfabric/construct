import { webAddress } from './external';

/** What the Glasshouse page and the main process say to each other, and the rules for addresses. */

/** A box in the window's own pixels, measured from its top left corner. */
export interface Bounds {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

/** Why the page below the address bar is not shown. */
export interface GlasshouseProblem {
  readonly kind: 'not-found' | 'refused' | 'offline' | 'certificate' | 'crashed' | 'other';
}

export interface GlasshouseState {
  /** The address the page is on, or the one it is going to. Empty before the first one. */
  readonly address: string;
  readonly loading: boolean;
  readonly canGoBack: boolean;
  readonly canGoForward: boolean;
  /** `true` when neither the built-in address nor the environment gives a start address. */
  readonly noStartAddress: boolean;
  readonly problem: GlasshouseProblem | null;
}

export type GoResult = 'opened' | 'refused';

export interface GlasshouseApi {
  /** Shows the web page in this box, creating it on the first call. */
  readonly showGlasshouse: (bounds: Bounds) => Promise<GlasshouseState>;
  /** Takes the web page off the window; it keeps running. */
  readonly hideGlasshouse: () => Promise<void>;
  readonly goGlasshouse: (address: string) => Promise<GoResult>;
  readonly stepGlasshouse: (step: 'back' | 'forward' | 'reload') => Promise<void>;
  /** The listener gets every change of the state; the result stops it. */
  readonly onGlasshouseState: (listener: (state: GlasshouseState) => void) => () => void;
}

export const GLASSHOUSE_SHOW_CHANNEL = 'glasshouse:show';
export const GLASSHOUSE_HIDE_CHANNEL = 'glasshouse:hide';
export const GLASSHOUSE_GO_CHANNEL = 'glasshouse:go';
export const GLASSHOUSE_STEP_CHANNEL = 'glasshouse:step';
export const GLASSHOUSE_STATE_CHANNEL = 'glasshouse:state';

/** The environment variable that replaces the built-in start address. */
export const START_ADDRESS_VARIABLE = 'CONSTRUCT_GLASSHOUSE_URL';

/** The address the page opens when the environment says nothing. */
export const builtInStartAddress = 'http://constructorfabric.com/';

/** The session of the page: its own storage, kept on disk, apart from the app's. */
export const GLASSHOUSE_PARTITION = 'persist:glasshouse';

const schemePrefix = /^([a-z][a-z0-9+.-]*):/i;
// `host:8080` reads like a scheme to the pattern above; a port is digits and then the end or a path.
const portAfterHost = /^:\d+(?:[/?#]|$)/;

/**
 * The web address a person typed, or `null` when it is not one that opens. An address with no
 * scheme gets `https://`; only `http` and `https` open, whoever asks.
 */
export function typedAddress(raw: unknown): string | null {
  if (typeof raw !== 'string') {
    return null;
  }
  const text = raw.trim();
  if (text === '') {
    return null;
  }
  const scheme = schemePrefix.exec(text);
  const hasScheme = scheme !== null && !portAfterHost.test(text.slice(scheme[1]?.length ?? 0));
  let url: URL;
  try {
    url = new URL(hasScheme ? text : `https://${text}`);
  } catch {
    return null;
  }
  // The one gate on web addresses is the one the system browser's link uses.
  return url.hostname === '' ? null : webAddress(url.href);
}

/**
 * The start address: the environment's when it is a web address; none (`null`) when the variable
 * is set to an empty text; else the built-in one. A value that is set but is not a web address is
 * ignored like an unset one, so the built-in address opens.
 */
export function startAddress(
  env: Readonly<Record<string, string | undefined>>,
  builtIn: string = builtInStartAddress,
): string | null {
  const fromEnv = env[START_ADDRESS_VARIABLE];
  if (fromEnv?.trim() === '') {
    return null;
  }
  return typedAddress(fromEnv) ?? typedAddress(builtIn);
}

/**
 * The User-Agent the page sends: the browser's own, with the Electron token and the app's own
 * (`<name>/<version>`) taken out and `Construct/<version>` put last. The `Chrome/<version>`
 * token stays: sites read it.
 */
export function glasshouseUserAgent(base: string, version: string): string {
  const keep = base
    .split(' ')
    .filter((token) => !token.startsWith('Electron/') && !token.endsWith(`/${version}`))
    .join(' ');
  return `${keep} Construct/${version}`;
}

/** What a failed load means for the person, by Chromium's error code. */
export function problemOf(code: number): GlasshouseProblem {
  if (code <= -200 && code >= -299) {
    return { kind: 'certificate' };
  }
  switch (code) {
    case -105:
      return { kind: 'not-found' };
    case -102:
      return { kind: 'refused' };
    case -106:
      return { kind: 'offline' };
    default:
      return { kind: 'other' };
  }
}
