import { describe, expect, it } from 'vitest';
import { webAddress } from '../src/ui/markdown/web-address';

describe('the renderer’s web address filter', () => {
  it('lets web addresses through and nothing else', () => {
    expect(webAddress('https://example.com/docs')).toBe('https://example.com/docs');
    for (const raw of ['javascript:alert(1)', 'file:///etc/hosts', '/docs', undefined]) {
      expect(webAddress(raw), String(raw)).toBeNull();
    }
  });
});
