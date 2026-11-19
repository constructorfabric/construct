import { describe, expect, it, vi } from 'vitest';
import { createCopyHandler, createLinkHandler, createPaneHandler } from '../src/external-calls';

describe('createLinkHandler', () => {
  it('opens a web address and says so', async () => {
    const open = vi.fn(() => Promise.resolve());
    expect(await createLinkHandler(open)('https://example.com/docs')).toBe(true);
    expect(open).toHaveBeenCalledExactlyOnceWith('https://example.com/docs');
  });

  it('opens nothing else', async () => {
    const open = vi.fn(() => Promise.resolve());
    const handler = createLinkHandler(open);
    for (const url of [
      'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility',
      'file:///etc/hosts',
      'javascript:alert(1)',
      '/docs',
      42,
      undefined,
    ]) {
      expect(await handler(url), String(url)).toBe(false);
    }
    expect(open).not.toHaveBeenCalled();
  });

  it('refuses, and does not throw, when the browser cannot be started', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const handler = createLinkHandler(() => Promise.reject(new Error('no browser')));
    expect(await handler('https://example.com')).toBe(false);
    expect(error).toHaveBeenCalledOnce();
  });
});

describe('createPaneHandler', () => {
  it('opens the pane of a name on the list and says so', async () => {
    const open = vi.fn(() => Promise.resolve());
    await expect(createPaneHandler(open)('accessibility')).resolves.toBe(true);
    expect(open).toHaveBeenCalledExactlyOnceWith(
      'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility',
    );
  });

  it('opens nothing for an address or a name that is not on the list', async () => {
    const open = vi.fn(() => Promise.resolve());
    const handler = createPaneHandler(open);
    for (const pane of ['https://example.com', 'file:///etc/hosts', 'nothing', 5, undefined]) {
      await expect(handler(pane)).resolves.toBe(false);
    }
    expect(open).not.toHaveBeenCalled();
  });

  it('refuses, and does not throw, when System Settings cannot be opened', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const handler = createPaneHandler(() => Promise.reject(new Error('no settings')));
    await expect(handler('automation')).resolves.toBe(false);
    expect(error).toHaveBeenCalledOnce();
  });
});

describe('createCopyHandler', () => {
  it('writes a text and says so', () => {
    const write = vi.fn();
    expect(createCopyHandler(write)('A-1: 1 h\nB-2: 20 min')).toBe(true);
    expect(write).toHaveBeenCalledExactlyOnceWith('A-1: 1 h\nB-2: 20 min');
  });

  it('writes nothing that is not a text of a fitting size', () => {
    const write = vi.fn();
    const handler = createCopyHandler(write);
    for (const text of ['', 'x'.repeat(64 * 1024 + 1), 42, undefined, { text: 'a' }]) {
      expect(handler(text), String(text)).toBe(false);
    }
    expect(write).not.toHaveBeenCalled();
  });

  it('refuses, and does not throw, when the clipboard cannot be written', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const handler = createCopyHandler(() => {
      throw new Error('no clipboard');
    });
    expect(handler('a')).toBe(false);
    expect(error).toHaveBeenCalledOnce();
  });
});
