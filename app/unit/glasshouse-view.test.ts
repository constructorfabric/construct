import { beforeEach, describe, expect, it, vi } from 'vitest';

// Stand-ins for the parts of Electron the page's view uses; each test reads what they were told.
const made = vi.hoisted(() => ({
  views: [] as { loaded: string[]; visible: boolean[] }[],
}));

vi.mock('electron', () => {
  const session = {
    getUserAgent: () => 'Mozilla/5.0 Chrome/1.0 Electron/1.0 app/1.0.0',
    setUserAgent: () => undefined,
    setPermissionRequestHandler: () => undefined,
    setPermissionCheckHandler: () => undefined,
    setDevicePermissionHandler: () => undefined,
    webRequest: { onHeadersReceived: () => undefined },
    on: () => undefined,
  };
  class WebContentsView {
    readonly record = { loaded: [] as string[], visible: [] as boolean[] };
    readonly webContents = {
      setUserAgent: () => undefined,
      setWindowOpenHandler: () => undefined,
      on: () => undefined,
      getURL: () => '',
      isLoading: () => false,
      navigationHistory: { canGoBack: () => false, canGoForward: () => false },
      loadURL: (address: string) => {
        this.record.loaded.push(address);
        return Promise.resolve();
      },
    };
    constructor() {
      made.views.push(this.record);
    }
    setBounds = () => undefined;
    setVisible = (on: boolean) => {
      this.record.visible.push(on);
    };
  }
  return {
    app: { getVersion: () => '1.0.0' },
    session: { fromPartition: () => session },
    clipboard: { read: () => Promise.resolve([]), clear: () => undefined },
    ClipboardItem: class {},
    WebContentsView,
  };
});

const { Glasshouse } = await import('../src/glasshouse-view');

const bounds = { x: 0, y: 0, width: 100, height: 100 };

/** A window that records the capture protection it is given, and what is added to it. */
function standInWindow() {
  const protection: boolean[] = [];
  const children: unknown[] = [];
  const window = {
    setContentProtection: (on: boolean) => protection.push(on),
    isDestroyed: () => false,
    on: () => undefined,
    webContents: { on: () => undefined, send: () => undefined },
    contentView: { children, addChildView: (view: unknown) => children.push(view) },
  };
  return { window, protection };
}

const make = (platform: NodeJS.Platform, env: Record<string, string> = {}) => {
  const stand = standInWindow();
  const glasshouse = new Glasshouse(
    () => stand.window as never,
    { CONSTRUCT_GLASSHOUSE_URL: 'https://example.com/', ...env },
    platform,
  );
  return { ...stand, glasshouse };
};

beforeEach(() => {
  made.views.length = 0;
});

describe('the Glasshouse page, on a window stand-in', () => {
  it('protects the window from screenshots when the page is shown and frees it when it is hidden', () => {
    const { glasshouse, protection } = make('darwin');
    glasshouse.show(bounds);
    expect(protection).toEqual([true]);
    glasshouse.show(bounds);
    expect(protection).toEqual([true]);
    glasshouse.hide();
    expect(protection).toEqual([true, false]);
    glasshouse.show(bounds);
    expect(protection).toEqual([true, false, true]);
  });

  it('does not touch the window on Linux or Windows', () => {
    for (const platform of ['linux', 'win32'] as const) {
      const { glasshouse, protection } = make(platform);
      glasshouse.show(bounds);
      glasshouse.hide();
      expect(protection, platform).toEqual([]);
    }
  });

  it('on Linux, shows the notice first and opens the start address only when the page goes on', () => {
    const { glasshouse } = make('linux');
    const state = glasshouse.show(bounds);
    expect(state.screenshot).toBe('open');
    expect(state.waiting).toBe(true);
    expect(made.views[0]?.loaded).toEqual([]);
    glasshouse.step('continue');
    expect(made.views[0]?.loaded).toEqual(['https://example.com/']);
    expect(glasshouse.state().waiting).toBe(false);
  });

  it('on Linux, an address the person types opens at once and the start address is dropped', () => {
    const { glasshouse } = make('linux');
    glasshouse.show(bounds);
    glasshouse.go('https://typed.example/');
    glasshouse.step('continue');
    expect(made.views[0]?.loaded).toEqual(['https://typed.example/']);
  });

  it('on macOS and Windows, opens the start address at once, with nothing to wait for', () => {
    for (const platform of ['darwin', 'win32'] as const) {
      made.views.length = 0;
      const { glasshouse } = make(platform);
      expect(glasshouse.show(bounds).waiting, platform).toBe(false);
      expect(made.views[0]?.loaded, platform).toEqual(['https://example.com/']);
    }
  });
});
