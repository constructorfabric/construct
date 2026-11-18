import { describe, expect, it } from 'vitest';
import { formatSize } from '../src/ui/setup-text';

describe('a size', () => {
  it('is shown in bytes, kilobytes or megabytes', () => {
    expect(formatSize(0)).toBe('0 B');
    expect(formatSize(1023)).toBe('1023 B');
    expect(formatSize(1536)).toBe('1.5 KB');
    expect(formatSize(5 * 1024 * 1024)).toBe('5.0 MB');
  });
});
