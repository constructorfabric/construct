import { describe, expect, it, vi } from 'vitest';
import { createLinkHandler } from '../src/external-calls';

describe('createLinkHandler', () => {
  it('opens a web address and says so', async () => {
    const open = vi.fn(() => Promise.resolve());
    expect(await createLinkHandler(open)('https://example.com/docs')).toBe(true);
    expect(open).toHaveBeenCalledExactlyOnceWith('https://example.com/docs');
  });

  it('opens nothing else', async () => {
    const open = vi.fn(() => Promise.resolve());
    const handler = createLinkHandler(open);
    for (const url of ['file:///etc/hosts', 'javascript:alert(1)', '/docs', 42, undefined]) {
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
