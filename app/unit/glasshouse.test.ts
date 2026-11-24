import { describe, expect, it } from 'vitest';
import {
  ClipboardHold,
  ScreenGuard,
  pageSandbox,
  refusedShortcut,
  screenshotModeOf,
  withSandbox,
  builtInStartAddress,
  glasshouseUserAgent,
  problemOf,
  START_ADDRESS_VARIABLE,
  startAddress,
  typedAddress,
} from '../src/glasshouse';

describe('a typed address', () => {
  it('gets https when it has no scheme', () => {
    expect(typedAddress('constructor.app')).toBe('https://constructor.app/');
    expect(typedAddress('  example.com/a?b=1#c ')).toBe('https://example.com/a?b=1#c');
  });

  it('keeps http and https as typed', () => {
    expect(typedAddress('http://127.0.0.1:8080/x')).toBe('http://127.0.0.1:8080/x');
    expect(typedAddress('HTTPS://Example.com')).toBe('https://example.com/');
  });

  it('reads a port after a host as a port, not as a scheme', () => {
    expect(typedAddress('localhost:8080')).toBe('https://localhost:8080/');
    expect(typedAddress('example.com:443/path')).toBe('https://example.com/path');
  });

  it('refuses every other scheme', () => {
    for (const raw of [
      'file:///etc/hosts',
      'javascript:alert(1)',
      'data:text/html,hi',
      'ftp://example.com',
      'chrome://gpu',
      'blob:https://example.com/x',
    ]) {
      expect(typedAddress(raw), raw).toBeNull();
    }
  });

  it('refuses what is not an address', () => {
    for (const raw of ['', '   ', 'two words', 'https://', undefined, null, 5]) {
      expect(typedAddress(raw), String(raw)).toBeNull();
    }
  });
});

describe('the start address', () => {
  it('comes from the environment first', () => {
    const env = { [START_ADDRESS_VARIABLE]: 'localhost:3000' };
    expect(startAddress(env, 'https://built-in.example')).toBe('https://localhost:3000/');
  });

  it('falls back to the built-in one when the environment has none or a bad one', () => {
    expect(startAddress({}, 'https://built-in.example')).toBe('https://built-in.example/');
    expect(startAddress({ [START_ADDRESS_VARIABLE]: 'file:///x' }, 'built-in.example')).toBe(
      'https://built-in.example/',
    );
  });

  it('is none when neither is set', () => {
    expect(startAddress({}, '')).toBeNull();
  });

  it('is none when the environment sets the variable to an empty text', () => {
    expect(startAddress({ [START_ADDRESS_VARIABLE]: '' }, 'https://built-in.example')).toBeNull();
  });

  it('is the built-in address, a web address, when the environment says nothing', () => {
    expect(builtInStartAddress).toBe('http://constructorfabric.com/');
    expect(startAddress({})).toBe('http://constructorfabric.com/');
  });
});

describe('the User-Agent of the page', () => {
  const base =
    'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) ConstructBrowser/1.0.0 Chrome/150.0.1.2 Electron/45.2.3 Safari/537.36';

  it('names Construct, keeps Chrome and drops Electron and the app’s own token', () => {
    expect(glasshouseUserAgent(base, '1.0.0')).toBe(
      'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.1.2 Safari/537.36 Construct/1.0.0',
    );
  });
});

describe('what a failed load means', () => {
  it('tells a certificate from an address that is not found', () => {
    expect(problemOf(-202).kind).toBe('certificate');
    expect(problemOf(-200).kind).toBe('certificate');
    expect(problemOf(-105).kind).toBe('not-found');
    expect(problemOf(-102).kind).toBe('refused');
    expect(problemOf(-106).kind).toBe('offline');
    expect(problemOf(-7).kind).toBe('other');
  });
});

describe('the screenshot handling of each system', () => {
  it('hides the window on macOS, tells the person on Linux and does nothing on Windows', () => {
    expect(screenshotModeOf('darwin')).toBe('protected');
    expect(screenshotModeOf('linux')).toBe('open');
    expect(screenshotModeOf('win32')).toBe('none');
  });

  const standIn = () => {
    const calls: boolean[] = [];
    return {
      calls,
      window: { setContentProtection: (on: boolean) => calls.push(on), isDestroyed: () => false },
    };
  };

  it('protects the window while the page is shown and frees it when another page is shown', () => {
    const { calls, window } = standIn();
    const guard = new ScreenGuard('protected');
    guard.show(window);
    guard.show(window);
    expect(calls).toEqual([true]);
    guard.hide();
    guard.hide();
    expect(calls).toEqual([true, false]);
  });

  it('leaves the window alone where the system has no protection', () => {
    for (const mode of ['open', 'none'] as const) {
      const { calls, window } = standIn();
      const guard = new ScreenGuard(mode);
      guard.show(window);
      guard.hide();
      expect(calls).toEqual([]);
    }
  });

  it('does not touch a window that is gone', () => {
    const calls: boolean[] = [];
    const guard = new ScreenGuard('protected');
    guard.show({ setContentProtection: (on) => calls.push(on), isDestroyed: () => true });
    guard.hide();
    expect(calls).toEqual([true]);
  });
});

describe('the clipboard hold', () => {
  const store = (options: { restoreFails?: boolean } = {}) => {
    const log: string[] = [];
    return {
      log,
      save: () => {
        log.push('save');
        return Promise.resolve('before');
      },
      restore: (saved: string) => {
        log.push(`restore ${saved}`);
        return options.restoreFails === true ? Promise.reject(new Error('no')) : Promise.resolve();
      },
      clear: () => {
        log.push('clear');
      },
    };
  };

  it('saves once and puts the clipboard back when the page that had the focus loses it', async () => {
    const fake = store();
    const hold = new ClipboardHold(fake);
    hold.take();
    hold.engage();
    hold.engage();
    await hold.release();
    expect(fake.log).toEqual(['save', 'restore before']);
  });

  it('puts nothing back when the page only had a hover, and saves again the next time', async () => {
    const fake = store();
    const hold = new ClipboardHold(fake);
    hold.take();
    await hold.release();
    hold.engage();
    await hold.release();
    await hold.release();
    expect(fake.log).toEqual(['save', 'save', 'restore before']);
  });

  it('reads the clipboard again while the page has no focus, and no more once it has', async () => {
    let time = 0;
    let held = 'one';
    const hold = new ClipboardHold(
      {
        save: () => Promise.resolve(held),
        restore: (saved: string) => {
          restored.push(saved);
          return Promise.resolve();
        },
        clear: () => undefined,
      },
      undefined,
      () => time,
      100,
    );
    const restored: string[] = [];
    hold.take();
    await Promise.resolve();
    await Promise.resolve();
    held = 'two';
    time = 50;
    hold.take();
    await Promise.resolve();
    await Promise.resolve();
    time = 150;
    hold.take();
    await Promise.resolve();
    await Promise.resolve();
    held = 'three';
    time = 300;
    hold.engage();
    await Promise.resolve();
    await Promise.resolve();
    held = 'four';
    time = 900;
    hold.engage();
    await Promise.resolve();
    await Promise.resolve();
    await hold.release();
    expect(restored).toEqual(['three']);
  });

  it('puts nothing back when the page never got input', async () => {
    const fake = store();
    await new ClipboardHold(fake).release();
    expect(fake.log).toEqual([]);
  });

  it('waits for a put-back in flight before it saves again', async () => {
    const order: string[] = [];
    let finish = (): void => undefined;
    const hold = new ClipboardHold({
      save: () => {
        order.push('save');
        return Promise.resolve('x');
      },
      restore: () =>
        new Promise<void>((resolve) => {
          order.push('restore starts');
          finish = () => {
            order.push('restore ends');
            resolve();
          };
        }),
      clear: () => undefined,
    });
    hold.engage();
    await Promise.resolve();
    const done = hold.release();
    hold.engage();
    await new Promise((resolve) => setTimeout(resolve, 10));
    finish();
    await done;
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(order).toEqual(['save', 'restore starts', 'restore ends', 'save']);
  });

  it('clears the clipboard and reports when the put-back fails, so the page text does not stay', async () => {
    const fake = store({ restoreFails: true });
    const reported: unknown[] = [];
    const hold = new ClipboardHold(fake, (error) => reported.push(error));
    hold.engage();
    await hold.release();
    expect(fake.log).toEqual(['save', 'restore before', 'clear']);
    expect(reported).toHaveLength(1);
  });

  it('clears the clipboard when it could not be saved', async () => {
    const log: string[] = [];
    const hold = new ClipboardHold({
      save: () => Promise.reject(new Error('unreadable')),
      restore: () => Promise.resolve(),
      clear: () => {
        log.push('clear');
      },
    });
    hold.engage();
    await hold.release();
    expect(log).toEqual(['clear']);
  });
});

describe('the shortcuts restricted mode refuses', () => {
  const press = (
    platform: NodeJS.Platform,
    code: string,
    extra: Partial<Parameters<typeof refusedShortcut>[0]> = {},
  ) =>
    refusedShortcut(
      {
        type: 'keyDown',
        key: code.replace('Key', '').toLowerCase(),
        code,
        control: false,
        meta: false,
        shift: false,
        alt: false,
        ...extra,
      },
      platform,
    );

  it('refuses the developer tools keys of each system', () => {
    expect(press('linux', 'F12')).toBe('tools');
    expect(press('linux', 'KeyI', { control: true, shift: true })).toBe('tools');
    expect(press('win32', 'KeyJ', { control: true, shift: true })).toBe('tools');
    expect(press('darwin', 'KeyC', { meta: true, shift: true })).toBe('tools');
    expect(press('darwin', 'KeyI', { meta: true, alt: true, key: 'ˆ' })).toBe('tools');
  });

  it('refuses print with the key of the system', () => {
    expect(press('linux', 'KeyP', { control: true })).toBe('print');
    expect(press('darwin', 'KeyP', { meta: true })).toBe('print');
  });

  it('follows the typed letter for print, not the place of the key', () => {
    expect(press('linux', 'KeyL', { control: true, key: 'p' })).toBe('print');
    expect(press('linux', 'KeyP', { control: true, key: 'r' })).toBeNull();
  });

  it('lets other chords through: Ctrl+P on a Mac, AltGr combinations, plain keys, releases', () => {
    expect(press('darwin', 'KeyP', { control: true })).toBeNull();
    expect(press('linux', 'KeyI', { control: true, shift: true, alt: true })).toBeNull();
    expect(press('linux', 'KeyC', { control: true })).toBeNull();
    expect(press('linux', 'KeyP')).toBeNull();
    expect(press('linux', 'F12', { type: 'keyUp' })).toBeNull();
  });
});

describe('the sandbox of the page', () => {
  it('leaves out modals, the one flag that lets a page print, and keeps what sites need', () => {
    const flags = pageSandbox.split(' ');
    expect(flags[0]).toBe('sandbox');
    expect(flags).not.toContain('allow-modals');
    expect(flags).toEqual(
      expect.arrayContaining(['allow-scripts', 'allow-same-origin', 'allow-forms']),
    );
  });

  it('adds to the policy a site sent, whatever the case of the header name', () => {
    for (const name of ['content-security-policy', 'Content-Security-Policy']) {
      expect(withSandbox({ [name]: ["default-src 'self'"], 'x-other': ['1'] })).toEqual({
        'x-other': ['1'],
        'Content-Security-Policy': ["default-src 'self'", pageSandbox],
      });
    }
    expect(withSandbox({})).toEqual({ 'Content-Security-Policy': [pageSandbox] });
  });
});
