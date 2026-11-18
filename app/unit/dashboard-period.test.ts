import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { dayBounds, mondayOf, weekBounds } from '../src/ui/dashboard-period';

const zone = process.env.TZ;

describe('the period of the dashboard', () => {
  beforeEach(() => {
    process.env.TZ = 'UTC';
  });
  afterEach(() => {
    if (zone === undefined) {
      delete process.env.TZ;
    } else {
      process.env.TZ = zone;
    }
  });

  it('starts a week on Monday, also for a Sunday', () => {
    expect(mondayOf(new Date(2026, 10, 18))).toEqual(new Date(2026, 10, 16));
    expect(mondayOf(new Date(2026, 10, 22))).toEqual(new Date(2026, 10, 16));
    expect(mondayOf(new Date(2026, 10, 16))).toEqual(new Date(2026, 10, 16));
  });

  it('gives seven consecutive days that each end where the next begins', () => {
    const week = weekBounds(new Date(2026, 10, 18, 15, 30));
    expect(week.map((d) => d.label)).toEqual([
      '2026-11-16',
      '2026-11-17',
      '2026-11-18',
      '2026-11-19',
      '2026-11-20',
      '2026-11-21',
      '2026-11-22',
    ]);
    week.slice(1).forEach((d, index) => {
      expect(d.start).toBe(week[index]?.end);
    });
  });

  it('writes a day as whole minutes in UTC', () => {
    expect(dayBounds(new Date(2026, 10, 18, 9))).toEqual({
      label: '2026-11-18',
      start: '2026-11-18T00:00:00Z',
      end: '2026-11-19T00:00:00Z',
    });
  });

  it('keeps a day whole when the clocks change: 25 hours in Berlin on 25 October 2026', () => {
    process.env.TZ = 'Europe/Berlin';
    expect(dayBounds(new Date(2026, 9, 25, 12))).toEqual({
      label: '2026-10-25',
      start: '2026-10-24T22:00:00Z',
      end: '2026-10-25T23:00:00Z',
    });
    // The week around it still has seven days, Monday to Sunday.
    expect(weekBounds(new Date(2026, 9, 25)).map((d) => d.label)).toEqual([
      '2026-10-19',
      '2026-10-20',
      '2026-10-21',
      '2026-10-22',
      '2026-10-23',
      '2026-10-24',
      '2026-10-25',
    ]);
  });
});
