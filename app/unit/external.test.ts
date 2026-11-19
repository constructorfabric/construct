import { describe, expect, it } from 'vitest';
import { systemPaneAddress, webAddress } from '../src/external';

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
      'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility',
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

describe('systemPaneAddress', () => {
  it.each([
    ['accessibility', 'Privacy_Accessibility'],
    ['screen_recording', 'Privacy_ScreenCapture'],
    ['automation', 'Privacy_Automation'],
    ['input_monitoring', 'Privacy_ListenEvent'],
  ])('gives the whole address of the %s pane', (name, pane) => {
    expect(systemPaneAddress(name)).toBe(
      `x-apple.systempreferences:com.apple.preference.security?${pane}`,
    );
  });

  it('refuses every name that is not on the list, and every address', () => {
    for (const raw of [
      'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility',
      'https://example.com',
      'file:///etc/hosts',
      '',
      'Accessibility',
      'toString',
      '__proto__',
      'constructor',
    ]) {
      expect(systemPaneAddress(raw), raw).toBeNull();
    }
  });

  it('refuses what is not text', () => {
    for (const raw of [undefined, null, 1, {}, ['accessibility']]) {
      expect(systemPaneAddress(raw)).toBeNull();
    }
  });
});
