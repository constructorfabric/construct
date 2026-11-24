import { describe, expect, it } from 'vitest';
import { disabledChromiumFeatures } from '../src/chromium-features';

describe('the Chromium features the app turns off', () => {
  it('turns off Skia Graphite on macOS, where it draws the glyph atlas of xterm.js wrongly', () => {
    expect(disabledChromiumFeatures('darwin')).toBe('SkiaGraphite');
  });

  it('turns off nothing on the other platforms', () => {
    expect(disabledChromiumFeatures('linux')).toBeNull();
    expect(disabledChromiumFeatures('win32')).toBeNull();
  });
});
