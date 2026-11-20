import path from 'node:path';
import { app, session, WebContentsView } from 'electron';
import type { BrowserWindow, Session } from 'electron';
import {
  GLASSHOUSE_PARTITION,
  GLASSHOUSE_STATE_CHANNEL,
  glasshouseUserAgent,
  problemOf,
  startAddress,
  typedAddress,
} from './glasshouse';
import type { Bounds, GlasshouseProblem, GlasshouseState, GoResult } from './glasshouse';

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
  return userAgent;
}

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

  constructor(
    private readonly window: () => BrowserWindow | null,
    private readonly env: Readonly<Record<string, string | undefined>>,
  ) {}

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
  }

  go(raw: unknown): GoResult {
    const address = typedAddress(raw);
    if (address === null) {
      return 'refused';
    }
    this.open(address);
    return 'opened';
  }

  step(raw: unknown): void {
    const contents = this.view?.webContents;
    if (contents === undefined) {
      return;
    }
    if (raw === 'back' || raw === 'forward') {
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
    // Electron asks where to save the file, in one dialog that starts from the file's name.
    ses.on('will-download', (_event, item) => {
      item.setSaveDialogOptions({ defaultPath: path.basename(item.getFilename()) });
    });
    // No preload, no Node, a sandbox and a session of its own: the page reaches nothing of the app.
    const view = new WebContentsView({
      webPreferences: {
        session: ses,
        contextIsolation: true,
        nodeIntegration: false,
        sandbox: true,
      },
    });
    const contents = view.webContents;
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
    if (start !== null) {
      this.open(start);
    }
    return view;
  }
}
