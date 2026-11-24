import { app, clipboard, ClipboardItem, session, WebContentsView } from 'electron';
import type { BrowserWindow, ClipboardBookmark, Session } from 'electron';
import {
  BLOCKED_NOTICE_MS,
  ClipboardHold,
  GLASSHOUSE_PARTITION,
  ScreenGuard,
  isFocusInput,
  refusedShortcut,
  screenshotModeOf,
  GLASSHOUSE_STATE_CHANNEL,
  glasshouseUserAgent,
  problemOf,
  startAddress,
  typedAddress,
  withSandbox,
} from './glasshouse';
import type {
  BlockedAction,
  Bounds,
  GlasshouseProblem,
  GlasshouseState,
  GoResult,
} from './glasshouse';

const isBounds = (raw: unknown): raw is Bounds => {
  if (typeof raw !== 'object' || raw === null) {
    return false;
  }
  return ['x', 'y', 'width', 'height'].every((key) => {
    const value: unknown = Reflect.get(raw, key);
    return typeof value === 'number' && Number.isFinite(value);
  });
};

/** Makes the page's session refuse what the page must never get, and name the browser in its requests. */
function configureSession(ses: Session): string {
  const userAgent = glasshouseUserAgent(ses.getUserAgent(), app.getVersion());
  ses.setUserAgent(userAgent);
  // Camera, microphone, location, notifications and the rest are refused without a prompt.
  ses.setPermissionRequestHandler((_contents, _permission, callback) => {
    callback(false);
  });
  ses.setPermissionCheckHandler(() => false);
  ses.setDevicePermissionHandler(() => false);
  // A document without `allow-modals` cannot print: Chromium ignores `window.print()` there.
  ses.webRequest.onHeadersReceived((details, callback) => {
    if (details.resourceType !== 'mainFrame' && details.resourceType !== 'subFrame') {
      callback({});
      return;
    }
    callback({ responseHeaders: withSandbox(details.responseHeaders ?? {}) });
  });
  return userAgent;
}

/** What the clipboard held: every type of every item, and a text that tells two clipboards apart. */
interface Saved {
  readonly items: readonly Record<string, Blob | ClipboardBookmark>[];
  readonly fingerprint: string;
}

async function readClipboard(): Promise<Saved> {
  const items: Record<string, Blob | ClipboardBookmark>[] = [];
  const parts: string[] = [];
  for (const item of await clipboard.read()) {
    const entry: Record<string, Blob | ClipboardBookmark> = {};
    for (const type of item.types) {
      // A type that cannot be read is left out of the copy; the rest is kept.
      const value = await item.getType(type).catch(() => null);
      if (value === null) {
        continue;
      }
      entry[type] = value;
      parts.push(
        value instanceof Blob
          ? `${type}:${value.size}:${value.type.startsWith('text/') ? await value.text() : ''}`
          : `${type}:${JSON.stringify(value)}`,
      );
    }
    if (Object.keys(entry).length > 0) {
      items.push(entry);
    }
  }
  return { items, fingerprint: parts.join('\n') };
}

/** The system clipboard, kept and put back with Electron's own `clipboard`. */
const systemClipboard = {
  save: readClipboard,
  async restore(saved: Saved): Promise<void> {
    // What it holds already (the page copied nothing) is left alone.
    if ((await readClipboard()).fingerprint === saved.fingerprint) {
      return;
    }
    if (saved.items.length === 0) {
      clipboard.clear();
      return;
    }
    await clipboard.write(saved.items.map((entry) => new ClipboardItem(entry)));
  },
  clear: () => {
    clipboard.clear();
  },
};

/**
 * The web page of the Glasshouse page: one view that lives as long as the app does, so it keeps
 * running while another page is open. The page's own window has no connection to it but the box
 * it is shown in.
 */
export class Glasshouse {
  private view: WebContentsView | null = null;
  private problem: GlasshouseProblem | null = null;
  // What was last asked for: a failed or crashed load is tried again from here.
  private attempt = '';
  private shown = false;
  // The start address waits here while the notice about the screenshot shows.
  private pendingStart: string | null = null;
  private blocked: BlockedAction | null = null;
  private blockedTimer: NodeJS.Timeout | null = null;
  private readonly clipboard = new ClipboardHold(systemClipboard, (error) => {
    console.error('the clipboard could not be put back; it was cleared', error);
  });
  private readonly screen: ScreenGuard;
  private readonly screenshot;
  private readonly watched = new WeakSet<BrowserWindow>();

  constructor(
    private readonly window: () => BrowserWindow | null,
    private readonly env: Readonly<Record<string, string | undefined>>,
    private readonly platform: NodeJS.Platform = process.platform,
  ) {
    this.screenshot = screenshotModeOf(platform);
    this.screen = new ScreenGuard(this.screenshot);
  }

  state(): GlasshouseState {
    const contents = this.view?.webContents;
    const history = contents?.navigationHistory;
    return {
      address: contents?.getURL() || this.attempt,
      loading: contents?.isLoading() ?? false,
      canGoBack: history?.canGoBack() ?? false,
      canGoForward: history?.canGoForward() ?? false,
      noStartAddress: startAddress(this.env) === null,
      problem: this.problem,
      screenshot: this.screenshot,
      waiting: this.pendingStart !== null,
      blocked: this.blocked,
    };
  }

  show(rawBounds: unknown): GlasshouseState {
    const window = this.window();
    if (window === null || !isBounds(rawBounds)) {
      return this.state();
    }
    const view = this.view ?? this.create();
    const bounds = {
      x: Math.round(rawBounds.x),
      y: Math.round(rawBounds.y),
      width: Math.max(0, Math.round(rawBounds.width)),
      height: Math.max(0, Math.round(rawBounds.height)),
    };
    view.setBounds(bounds);
    this.watch(window);
    this.screen.show(window);
    this.shown = true;
    view.setVisible(this.problem === null);
    if (!window.contentView.children.includes(view)) {
      window.contentView.addChildView(view);
    }
    return this.state();
  }

  hide(): void {
    this.shown = false;
    this.view?.setVisible(false);
    this.screen.hide();
    void this.clipboard.release();
  }

  go(raw: unknown): GoResult {
    const address = typedAddress(raw);
    if (address === null) {
      return 'refused';
    }
    this.pendingStart = null;
    this.open(address);
    return 'opened';
  }

  step(raw: unknown): void {
    const contents = this.view?.webContents;
    if (contents === undefined) {
      return;
    }
    if (raw === 'continue') {
      const start = this.pendingStart;
      this.pendingStart = null;
      if (start === null) {
        return;
      }
      this.open(start);
    } else if (raw === 'back' || raw === 'forward') {
      // The page it goes to replaces a failed one; a load that fails again sets the problem again.
      this.setProblem(null);
      if (raw === 'back') {
        contents.navigationHistory.goBack();
      } else {
        contents.navigationHistory.goForward();
      }
      this.publish();
    } else if (raw === 'reload') {
      if (this.problem !== null && this.attempt !== '') {
        this.open(this.attempt);
      } else {
        contents.reload();
      }
    }
  }

  /** The clipboard goes back at each moment the page may stop having the focus. */
  private watch(window: BrowserWindow): void {
    if (this.watched.has(window)) {
      return;
    }
    this.watched.add(window);
    const release = (): void => {
      void this.clipboard.release();
    };
    // Another app, or another window of the app, has the focus; or the window is going away.
    window.on('blur', release);
    window.on('close', release);
    // A click or a key in the app's own window (the address bar) means the page is not focused.
    window.webContents.on('input-event', (_event, input) => {
      if (isFocusInput(input.type)) {
        release();
      }
    });
  }

  private refuse(action: BlockedAction): void {
    this.blocked = action;
    if (this.blockedTimer !== null) {
      clearTimeout(this.blockedTimer);
    }
    this.blockedTimer = setTimeout(() => {
      this.blocked = null;
      this.blockedTimer = null;
      this.publish();
    }, BLOCKED_NOTICE_MS);
    this.blockedTimer.unref();
    this.publish();
  }

  private open(address: string): void {
    this.attempt = address;
    this.setProblem(null);
    void this.view?.webContents.loadURL(address).catch(() => {
      // A refused or aborted load is reported by `did-fail-load`.
    });
    this.publish();
  }

  private setProblem(problem: GlasshouseProblem | null): void {
    this.problem = problem;
    this.view?.setVisible(this.shown && problem === null);
  }

  private publish(): void {
    const window = this.window();
    if (window !== null && !window.isDestroyed()) {
      window.webContents.send(GLASSHOUSE_STATE_CHANNEL, this.state());
    }
  }

  private create(): WebContentsView {
    const ses = session.fromPartition(GLASSHOUSE_PARTITION);
    const userAgent = configureSession(ses);
    // A download is refused; nothing is saved, and the app's window says so.
    ses.on('will-download', (event) => {
      event.preventDefault();
      this.refuse('download');
    });
    // No preload, no Node, a sandbox and a session of its own: the page reaches nothing of the app.
    const view = new WebContentsView({
      webPreferences: {
        session: ses,
        contextIsolation: true,
        nodeIntegration: false,
        sandbox: true,
        // No developer tools for the page, whatever opens them: a shortcut, the menu or a script.
        devTools: false,
      },
    });
    const contents = view.webContents;
    contents.on('before-input-event', (event, input) => {
      const action = refusedShortcut(input, this.platform);
      if (action !== null) {
        event.preventDefault();
        this.refuse(action);
      }
    });
    contents.on('input-event', (_event, input) => {
      // The clipboard is read at the first sign of the page (a hover); a click or a key means it has the focus.
      if (isFocusInput(input.type)) {
        this.clipboard.engage();
      } else {
        this.clipboard.take();
      }
    });
    contents.on('console-message', (event) => {
      if (/Ignored call to 'print\(\)'/.test(event.message)) {
        this.refuse('print');
      }
    });
    contents.setUserAgent(userAgent);
    // A page that asks for a new window gets it in the same page.
    contents.setWindowOpenHandler(({ url }) => {
      const address = typedAddress(url);
      if (address !== null) {
        this.open(address);
      }
      return { action: 'deny' };
    });
    // A page's own navigation and a server's redirect both stop at an address that is not a web one:
    // what Chromium does with such a redirect differs by scheme, and some it hands to the system.
    const guard = (event: { url: string; preventDefault: () => void }): void => {
      if (typedAddress(event.url) === null) {
        event.preventDefault();
      }
    };
    contents.on('will-navigate', guard);
    contents.on('will-redirect', guard);
    contents.on('did-fail-load', (_event, code, _description, url, isMainFrame) => {
      // -3 is a load that another one replaced.
      if (isMainFrame && code !== -3) {
        this.attempt = url;
        this.setProblem(problemOf(code));
      }
      this.publish();
    });
    contents.on('render-process-gone', () => {
      void this.clipboard.release();
      this.setProblem({ kind: 'crashed' });
      this.publish();
    });
    contents.on('did-start-loading', () => {
      this.publish();
    });
    contents.on('did-stop-loading', () => {
      this.publish();
    });
    contents.on('did-navigate-in-page', () => {
      this.publish();
    });
    contents.on('did-navigate', () => {
      this.attempt = contents.getURL();
      this.publish();
    });
    this.view = view;
    const start = startAddress(this.env);
    if (start !== null && this.screenshot === 'open') {
      // The person reads the notice about the screenshot first; the page asks to go on.
      this.pendingStart = start;
    } else if (start !== null) {
      this.open(start);
    }
    return view;
  }
}
