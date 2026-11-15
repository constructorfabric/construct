import { describe, expect, it } from 'vitest';
import { webAddress } from '../src/external';

describe('webAddress', () => {
  it('accepts https and http addresses', () => {
    expect(webAddress('https://example.com/docs?a=1#top')).toBe('https://example.com/docs?a=1#top');
    expect(webAddress('http://example.com')).toBe('http://example.com/');
  });

  it('refuses every other scheme', () => {
    for (const raw of [
      'javascript:alert(1)',
      'JavaScript:alert(1)',
      'file:///etc/hosts',
      'data:text/html,<p>x</p>',
      'mailto:someone@example.com',
      'ftp://example.com/file',
      'custom-app://open',
    ]) {
      expect(webAddress(raw), raw).toBeNull();
    }
  });

  it('refuses what is not an absolute address', () => {
    for (const raw of ['', '/docs', '../docs', 'example.com', '#top', 'http://', ' ']) {
      expect(webAddress(raw), raw).toBeNull();
    }
  });

  it('refuses what is not text', () => {
    for (const raw of [undefined, null, 42, {}, ['https://example.com']]) {
      expect(webAddress(raw)).toBeNull();
    }
  });
});
