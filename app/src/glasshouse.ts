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

/** What restricted mode refused, so the app's own window can say so. */
export type BlockedAction = 'download' | 'print' | 'tools';

/**
 * What the system lets restricted mode do about a screenshot of the page: `protected` (the window
 * is hidden from captures), `open` (it cannot be blocked, and the person is told), `none` (this
 * system has no handling).
 */
export type ScreenshotMode = 'protected' | 'open' | 'none';

export interface GlasshouseState {
  /** The address the page is on, or the one it is going to. Empty before the first one. */
  readonly address: string;
  readonly loading: boolean;
  readonly canGoBack: boolean;
  readonly canGoForward: boolean;
  /** `true` when neither the built-in address nor the environment gives a start address. */
  readonly noStartAddress: boolean;
  readonly problem: GlasshouseProblem | null;
  readonly screenshot: ScreenshotMode;
  /** `true` while the notice about the screenshot shows and the start address has not opened yet. */
  readonly waiting: boolean;
  /** The action refused a moment ago, or `null`. */
  readonly blocked: BlockedAction | null;
}

export type GoResult = 'opened' | 'refused';

export interface GlasshouseApi {
  /** Shows the web page in this box, creating it on the first call. */
  readonly showGlasshouse: (bounds: Bounds) => Promise<GlasshouseState>;
  /** Takes the web page off the window; it keeps running. */
  readonly hideGlasshouse: () => Promise<void>;
  readonly goGlasshouse: (address: string) => Promise<GoResult>;
  readonly stepGlasshouse: (step: 'back' | 'forward' | 'reload' | 'continue') => Promise<void>;
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

/** The screenshot handling of a system: macOS hides the window, Linux cannot, Windows has none. */
export function screenshotModeOf(platform: NodeJS.Platform): ScreenshotMode {
  if (platform === 'darwin') {
    return 'protected';
  }
  return platform === 'linux' ? 'open' : 'none';
}

/** The part of a window that hides it from screenshots and screen recordings. */
export interface Capturable {
  readonly setContentProtection: (enable: boolean) => void;
  readonly isDestroyed: () => boolean;
}

/**
 * Switches the capture protection of a window on while the page is shown and off when it is not.
 * The protection belongs to the whole window, so it follows the page: the other pages of the app
 * can be captured.
 */
export class ScreenGuard {
  private guarded: Capturable | null = null;

  constructor(private readonly mode: ScreenshotMode) {}

  show(window: Capturable): void {
    if (this.mode !== 'protected' || this.guarded === window) {
      return;
    }
    this.hide();
    window.setContentProtection(true);
    this.guarded = window;
  }

  hide(): void {
    const window = this.guarded;
    this.guarded = null;
    if (window !== null && !window.isDestroyed()) {
      window.setContentProtection(false);
    }
  }
}

/** A key press, as `before-input-event` reports it. */
export interface KeyPress {
  readonly type: string;
  readonly key: string;
  readonly code: string;
  readonly control: boolean;
  readonly meta: boolean;
  readonly shift: boolean;
  readonly alt: boolean;
}

/**
 * The action a shortcut would start that restricted mode refuses, or `null` for any other key.
 * Each system's own chord counts: Cmd on macOS, Ctrl elsewhere (Ctrl+P is "cursor up" on a Mac).
 */
export function refusedShortcut(key: KeyPress, platform: NodeJS.Platform): BlockedAction | null {
  if (key.type !== 'keyDown') {
    return null;
  }
  if (key.code === 'F12') {
    return 'tools';
  }
  const mac = platform === 'darwin';
  const command = mac ? key.meta && !key.control : key.control && !key.meta;
  // The physical key: Option changes the character a Mac key types, and Chromium binds the chord to the key.
  const toolsKey = ['KeyI', 'KeyJ', 'KeyC'].includes(key.code);
  if (toolsKey && command && ((key.shift && !key.alt) || (mac && key.alt && !key.shift))) {
    return 'tools';
  }
  return key.key.toLowerCase() === 'p' && command && !key.shift && !key.alt ? 'print' : null;
}

/** The response headers with the page's sandbox added to any policy the site sent, whatever the case of its name. */
export function withSandbox(headers: Readonly<Record<string, string[]>>): Record<string, string[]> {
  const result: Record<string, string[]> = {};
  let policies: string[] = [];
  for (const [name, values] of Object.entries(headers)) {
    if (name.toLowerCase() === 'content-security-policy') {
      policies = [...policies, ...values];
    } else {
      result[name] = values;
    }
  }
  result['Content-Security-Policy'] = [...policies, pageSandbox];
  return result;
}

/** How long a refusal stays on the screen. */
export const BLOCKED_NOTICE_MS = 5000;

/**
 * The CSP `sandbox` the page's documents get. It leaves out `allow-modals`, which is what lets a
 * page call `window.print()`; everything else a site needs stays.
 */
export const pageSandbox = [
  'sandbox',
  ...[
    'allow-downloads',
    'allow-forms',
    'allow-orientation-lock',
    'allow-pointer-lock',
    'allow-popups',
    'allow-popups-to-escape-sandbox',
    'allow-presentation',
    'allow-same-origin',
    'allow-scripts',
    'allow-storage-access-by-user-activation',
    'allow-top-navigation',
  ],
].join(' ');

/** Where the system clipboard is kept and put back. */
export interface ClipboardStore<T> {
  readonly save: () => Promise<T>;
  /** Puts `saved` back, unless the clipboard already holds it. */
  readonly restore: (saved: T) => Promise<void>;
  readonly clear: () => void;
}

/**
 * Keeps what the system clipboard held before the page was used and puts it back when the page
 * loses the focus, so text copied in the page never stays on the clipboard for another app.
 * `take` is called on any input the page gets (a hover too), so the clipboard is read well before
 * a click or a key can make the page copy; `engage` says the page really has the focus, and only
 * then is anything put back. When putting back fails the clipboard is cleared: the page's text
 * must not stay.
 */
export class ClipboardHold<T> {
  private saved: Promise<T> | null = null;
  private engaged = false;
  private releasing: Promise<void> = Promise.resolve();

  private savedAt = 0;

  constructor(
    private readonly store: ClipboardStore<T>,
    private readonly report: (error: unknown) => void = () => undefined,
    private readonly now: () => number = Date.now,
    /** How old a saved clipboard may get while the page has no focus before it is read again. */
    private readonly refreshMs = 50,
  ) {}

  /**
   * The page gets input: remember the clipboard, after any put-back in flight. Until the page has
   * the focus the copy is read again once it is `refreshMs` old, so a copy made elsewhere in the
   * meantime is the one that is kept; from then on it is not.
   */
  take(): void {
    if (this.engaged) {
      return;
    }
    const now = this.now();
    if (this.saved !== null && now - this.savedAt < this.refreshMs) {
      return;
    }
    this.savedAt = now;
    this.saved = this.releasing.then(() => this.store.save());
    // A failed save is dealt with at the put-back.
    this.saved.catch(() => undefined);
  }

  /** The page has the focus (a click or a key): it may copy now. */
  engage(): void {
    this.take();
    this.engaged = true;
  }

  /** The page lost the focus: put the clipboard back, if the page could have changed it. */
  release(): Promise<void> {
    const saved = this.saved;
    const engaged = this.engaged;
    this.saved = null;
    this.engaged = false;
    if (saved === null || !engaged) {
      return this.releasing;
    }
    this.releasing = this.releasing.then(async () => {
      try {
        await this.store.restore(await saved);
      } catch (error) {
        this.report(error);
        this.store.clear();
      }
    });
    return this.releasing;
  }
}

/** The input that means the page, or the app's own window, has the focus. */
export function isFocusInput(type: string): boolean {
  return ['mouseDown', 'keyDown', 'rawKeyDown', 'char', 'touchStart'].includes(type);
}
