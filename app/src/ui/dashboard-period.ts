import { addDays, format, startOfDay, startOfWeek } from 'date-fns';
import type { DayBounds, Period } from '../dashboard-protocol';
import { t } from '../translation';

/** The Monday of the week of `date`; the page's weeks run Monday to Sunday. */
export function mondayOf(date: Date): Date {
  return startOfWeek(date, { weekStartsOn: 1 });
}

/** The label of a day, `2026-11-18`, in the user's time zone. */
export function dayLabel(date: Date): string {
  return format(date, 'yyyy-MM-dd');
}

/** A moment as the agent reads it: UTC, a whole minute, `2026-11-17T23:00:00Z`. */
function utcMinute(date: Date): string {
  return `${date.toISOString().slice(0, 16)}:00Z`;
}

/** One day as the window counts it: from its first moment to the first moment of the next. */
export function dayBounds(date: Date): DayBounds {
  const start = startOfDay(date);
  return {
    label: dayLabel(start),
    start: utcMinute(start),
    end: utcMinute(startOfDay(addDays(date, 1))),
  };
}

/** The seven days, Monday to Sunday, of the week of `date`. */
export function weekBounds(date: Date): DayBounds[] {
  const first = mondayOf(date);
  return Array.from({ length: 7 }, (_, index) => dayBounds(addDays(first, index)));
}

/** A day as the page names it, `Nov 16`. */
export function shortDate(date: Date): string {
  return format(date, 'MMM d');
}

/** `1 h 5 min`, `45 min`: whole minutes, as the page shows time. */
export function formatMinutes(minutes: number): string {
  const whole = Math.round(minutes);
  return whole < 60
    ? t('DashboardPage.Minutes', { minutes: whole })
    : t('DashboardPage.HoursMinutes', { hours: Math.floor(whole / 60), minutes: whole % 60 });
}

/** The days of a period, as the document names it, counted from `anchor`, which stands for today. */
export function periodDays(period: Period, anchor: Date): DayBounds[] {
  const today = startOfDay(anchor);
  const run = (first: Date, count: number): DayBounds[] =>
    Array.from({ length: count }, (_, index) => dayBounds(addDays(first, index)));
  const month = (offset: number): DayBounds[] => {
    const first = new Date(today.getFullYear(), today.getMonth() + offset, 1);
    return run(first, new Date(first.getFullYear(), first.getMonth() + 1, 0).getDate());
  };
  switch (period) {
    case 'today':
      return run(today, 1);
    case 'this_week':
      return weekBounds(today);
    case 'last_week':
      return weekBounds(addDays(today, -7));
    case 'this_month':
      return month(0);
    case 'last_month':
      return month(-1);
    case 'last_7_days':
      return run(addDays(today, -6), 7);
    case 'last_30_days':
      return run(addDays(today, -29), 30);
  }
}
