import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { DashboardDocument, Period, WidgetKind } from '../src/dashboard-protocol';
import {
  countsDay,
  dayBounds,
  dayLabel,
  formatMinutes,
  mondayOf,
  pageLabel,
  periodAnchor,
  periodDays,
  rangeLabel,
  shortDate,
  weekBounds,
} from '../src/ui/dashboard-period';

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

  it('keeps a day whole when the clocks skip midnight: Cairo on 24 April 2026', () => {
    process.env.TZ = 'Africa/Cairo';
    // Midnight does not exist that day: the clocks go from 00:00 to 01:00.
    expect(dayBounds(new Date(2026, 3, 24, 12))).toEqual({
      label: '2026-04-24',
      start: '2026-04-23T22:00:00Z',
      end: '2026-04-24T21:00:00Z',
    });
    // Each day of the week ends exactly where the next one starts: the agent refuses overlaps.
    const week = weekBounds(new Date(2026, 3, 24));
    week.slice(1).forEach((day, index) => {
      expect(day.start).toBe(week[index]?.end);
    });
  });

  it('names a day as month and number', () => {
    expect(shortDate(new Date(2026, 10, 16))).toBe('Nov 16');
  });

  it('shows minutes rounded to whole minutes, as hours and minutes from an hour on', () => {
    expect(formatMinutes(0)).toBe('0 min');
    expect(formatMinutes(45.4)).toBe('45 min');
    expect(formatMinutes(45.6)).toBe('46 min');
    expect(formatMinutes(205)).toBe('3 h 25 min');
    expect(formatMinutes(60)).toBe('1 h 0 min');
  });
});

describe('the days of a period of the document', () => {
  const anchor = new Date(2026, 10, 18, 15, 30);
  const labels = (period: Parameters<typeof periodDays>[0]): string[] =>
    periodDays(period, anchor).map((d) => d.label);

  it('is the day itself for today', () => {
    expect(labels('today')).toEqual(['2026-11-18']);
  });

  it('is Monday to Sunday for this week and for last week', () => {
    expect(labels('this_week')).toHaveLength(7);
    expect(labels('this_week')[0]).toBe('2026-11-16');
    expect(labels('last_week')[0]).toBe('2026-11-09');
    expect(labels('last_week')[6]).toBe('2026-11-15');
  });

  it('is the whole calendar month for this month and for last month', () => {
    expect(labels('this_month')).toHaveLength(30);
    expect(labels('this_month')[0]).toBe('2026-11-01');
    expect(labels('this_month')[29]).toBe('2026-11-30');
    expect(labels('last_month')).toHaveLength(31);
    expect(labels('last_month')[0]).toBe('2026-10-01');
    expect(periodDays('last_month', new Date(2026, 0, 5))[0]?.label).toBe('2025-12-01');
  });

  it('counts back from the day for the last 7 and the last 30 days', () => {
    expect(labels('last_7_days')[0]).toBe('2026-11-12');
    expect(labels('last_7_days')[6]).toBe('2026-11-18');
    expect(labels('last_30_days')).toHaveLength(30);
    expect(labels('last_30_days')[0]).toBe('2026-10-20');
  });
});

describe('the period the page shows', () => {
  const page = (
    period: Period,
    widgets: readonly { kind: WidgetKind; period?: Period }[],
  ): DashboardDocument =>
    ({
      period,
      widgets: widgets.map((widget, index) => ({ id: `w${index}`, ...widget })),
    }) as unknown as DashboardDocument;
  const wednesday = new Date(2026, 10, 18);

  it('is a day when a widget counts a day, and a week when none does', () => {
    expect(countsDay(page('this_week', [{ kind: 'total' }]))).toBe(false);
    expect(countsDay(page('this_week', [{ kind: 'total', period: 'today' }]))).toBe(true);
    expect(countsDay(page('today', [{ kind: 'total' }]))).toBe(true);
    expect(countsDay(page('this_week', [{ kind: 'total' }, { kind: 'day_summary' }]))).toBe(true);
    expect(countsDay(page('last_30_days', [{ kind: 'bars_per_day' }]))).toBe(false);
    expect(countsDay(null)).toBe(false);
    expect(countsDay(page('this_week', []))).toBe(false);
    expect(countsDay(page('today', []))).toBe(false);
  });

  it('stands on the chosen day for a day, and on the last day of the week for a week', () => {
    const day = page('this_week', [{ kind: 'day_summary' }]);
    const week = page('this_week', [{ kind: 'total' }]);
    expect(dayLabel(periodAnchor(day, wednesday))).toBe('2026-11-18');
    expect(dayLabel(periodAnchor(week, wednesday))).toBe('2026-11-22');
    expect(dayLabel(periodAnchor(week, new Date(2026, 10, 16)))).toBe('2026-11-22');
    // The Sunday is the last day of its own week, not of the next.
    expect(dayLabel(periodAnchor(week, new Date(2026, 10, 22)))).toBe('2026-11-22');
    // A week across a month and a year.
    expect(dayLabel(periodAnchor(week, new Date(2026, 10, 30)))).toBe('2026-12-06');
    expect(dayLabel(periodAnchor(week, new Date(2026, 11, 30)))).toBe('2027-01-03');
    // The seven days up to that day are the week.
    expect(periodDays('last_7_days', periodAnchor(week, wednesday))[0]?.label).toBe('2026-11-16');
  });

  it('is named by its day, or by the first and the last day of its week', () => {
    expect(pageLabel(page('this_week', [{ kind: 'day_summary' }]), wednesday)).toBe('Nov 18, 2026');
    expect(pageLabel(page('this_week', [{ kind: 'total' }]), wednesday)).toBe(
      'Nov 16 – Nov 22, 2026',
    );
  });

  it('names a week across a month and a year by its own days', () => {
    const week = page('this_week', [{ kind: 'total' }]);
    expect(pageLabel(week, new Date(2026, 10, 30))).toBe('Nov 30 – Dec 6, 2026');
    expect(pageLabel(week, new Date(2026, 11, 31))).toBe('Dec 28 – Jan 3, 2027');
  });

  it('names the first and the last day of a period of a widget', () => {
    expect(rangeLabel(periodDays('today', wednesday))).toBe('Nov 18');
    expect(rangeLabel(periodDays('this_week', wednesday))).toBe('Nov 16 – Nov 22');
    expect(rangeLabel(periodDays('last_month', wednesday))).toBe('Oct 1 – Oct 31');
    // A week across a month and a year.
    expect(rangeLabel(periodDays('this_week', new Date(2026, 11, 30)))).toBe('Dec 28 – Jan 3');
  });
});
