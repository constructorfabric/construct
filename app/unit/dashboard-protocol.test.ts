import { describe, expect, it } from 'vitest';
import {
  dashboardNumbersLine,
  maxDays,
  parseDashboardInput,
  parseDashboardReply,
} from '../src/dashboard-protocol';

const day = { label: '2026-11-16', start: '2026-11-16T00:00:00Z', end: '2026-11-17T00:00:00Z' };

describe('parseDashboardInput', () => {
  it('takes the days and whose time, and nothing else', () => {
    expect(parseDashboardInput({ days: [day], sessions: 'own', extra: 1 })).toEqual({
      days: [day],
      sessions: 'own',
    });
  });

  it.each([
    ['not an object', 'x'],
    ['no days', { days: [], sessions: 'own' }],
    ['days that are not a list', { days: day, sessions: 'own' }],
    ['an unknown choice', { days: [day], sessions: 'everyone' }],
    ['a day without a label', { days: [{ ...day, label: '' }], sessions: 'own' }],
    ['a time with seconds', { days: [{ ...day, start: '2026-11-16T00:00:30Z' }], sessions: 'own' }],
    [
      'a time that is not UTC',
      { days: [{ ...day, end: '2026-11-17T00:00:00+02:00' }], sessions: 'own' },
    ],
    ['too many days', { days: Array.from({ length: maxDays + 1 }, () => day), sessions: 'own' }],
  ])('refuses %s', (_name, input) => {
    expect(parseDashboardInput(input)).toBeNull();
  });
});

describe('dashboardNumbersLine', () => {
  it('is the command and a JSON object with the days and the choice', () => {
    const line = dashboardNumbersLine({ days: [day], sessions: 'all' });
    expect(line.startsWith('dashboard-numbers ')).toBe(true);
    expect(JSON.parse(line.slice('dashboard-numbers '.length))).toEqual({
      days: [day],
      sessions: 'all',
    });
  });
});

describe('parseDashboardReply', () => {
  it('keeps the totals and the minutes of each day, and leaves the groups out', () => {
    const reply = JSON.stringify({
      total_minutes: 95.5,
      session_count: 3,
      hidden_project_count: 0,
      groups: [{ key: 'a project', minutes: 95.5, share: 1, session_count: 3 }],
      days: [{ label: '2026-11-16', minutes: 95.5 }],
    });
    expect(parseDashboardReply(reply)).toEqual({
      ok: true,
      totalMinutes: 95.5,
      sessions: 3,
      days: [{ label: '2026-11-16', minutes: 95.5 }],
    });
  });

  it('gives the text of an error', () => {
    expect(parseDashboardReply('{"error":"the dashboard numbers are not available"}')).toEqual({
      ok: false,
      error: 'the dashboard numbers are not available',
    });
  });

  it('throws on a reply that has no numbers', () => {
    expect(() => parseDashboardReply('{"status":"running"}')).toThrow('no numbers');
    expect(() => parseDashboardReply('{"total_minutes":1,"session_count":1,"days":[{}]}')).toThrow(
      'a day',
    );
  });
});
