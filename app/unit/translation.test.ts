import { describe, expect, it } from 'vitest';
import { t } from '../src/translation';

describe('t', () => {
  it('reads a text', () => {
    expect(t('App.Status')).toBe('Status');
  });

  it('fills the placeholders', () => {
    expect(t('StatusPage.Version', { version: '0.1.0', pid: 42 })).toBe('version 0.1.0, pid 42');
  });

  it('keeps a placeholder it has no value for', () => {
    expect(t('StatusPage.Version', { version: '0.1.0' })).toBe('version 0.1.0, pid {{pid}}');
  });
});
