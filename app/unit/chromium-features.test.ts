import { describe, expect, it } from 'vitest';
import { applyChromiumFeatures } from '../src/chromium-features';

const switchesOn = (platform: NodeJS.Platform): [string, string | undefined][] => {
  const switches: [string, string | undefined][] = [];
  applyChromiumFeatures(
    {
      appendSwitch: (name, value) => {
        switches.push([name, value]);
      },
    },
    platform,
  );
  return switches;
};

describe('the Chromium features the app turns off', () => {
  it('puts disable-features=SkiaGraphite on the command line on macOS', () => {
    expect(switchesOn('darwin')).toEqual([['disable-features', 'SkiaGraphite']]);
  });

  it('puts nothing on the command line on the other platforms', () => {
    expect(switchesOn('linux')).toEqual([]);
    expect(switchesOn('win32')).toEqual([]);
  });
});
