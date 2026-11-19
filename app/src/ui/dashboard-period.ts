import { addDays, format, startOfDay, startOfWeek } from 'date-fns';
import type { DashboardDocument, DayBounds, Period } from '../dashboard-protocol';
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

/** `count` days, one at least, from `first`. */
function run(first: Date, count: number): [DayBounds, ...DayBounds[]] {
  return [
    dayBounds(first),
    ...Array.from({ length: count - 1 }, (_, index) => dayBounds(addDays(first, index + 1))),
  ];
}

/** The seven days, Monday to Sunday, of the week of `date`. */
export function weekBounds(date: Date): [DayBounds, ...DayBounds[]] {
  return run(mondayOf(date), 7);
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
export function periodDays(period: Period, anchor: Date): [DayBounds, ...DayBounds[]] {
  const today = startOfDay(anchor);
  const month = (offset: number): [DayBounds, ...DayBounds[]] => {
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

/** Whether a widget of the document counts a single day, so that the page has to step by a day. */
export function countsDay(document: DashboardDocument | null): boolean {
  return (
    document !== null &&
    document.widgets.some(
      (widget) => widget.kind === 'day_summary' || (widget.period ?? document.period) === 'today',
    )
  );
}

/**
 * The day the periods of the page are counted from. A page that counts a day stands on the day
 * chosen; one that counts weeks at most stands on the last day of the chosen week, so that the
 * week, and the seven days up to it, are the week chosen. The month widgets follow that last day:
 * a month is not a week, so they cannot follow the week's own days, and the other widgets are the
 * ones the header names.
 */
export function periodAnchor(document: DashboardDocument | null, day: Date): Date {
  return countsDay(document) ? day : addDays(mondayOf(day), 6);
}

/** The days a page shows, as it names them: `Nov 18, 2026`, or `Nov 16 – Nov 22, 2026`. */
export function pageLabel(document: DashboardDocument | null, day: Date): string {
  const anchor = periodAnchor(document, day);
  if (countsDay(document)) {
    return format(anchor, 'MMM d, yyyy');
  }
  return `${shortDate(mondayOf(anchor))} – ${format(anchor, 'MMM d, yyyy')}`;
}

/** A day label, `2026-11-16`, as the date of that day in the user's time zone. */
export function dateOfLabel(label: string): Date | null {
  const [year, month, day] = label.split('-').map(Number);
  if (year === undefined || month === undefined || day === undefined) {
    return null;
  }
  return new Date(year, month - 1, day);
}

/** The first and the last day of a period, as a widget names them: `Nov 16 – Nov 22`. */
export function rangeLabel(days: readonly [DayBounds, ...DayBounds[]]): string {
  const first = days[0];
  const last = days.at(-1) ?? first;
  const name = ({ label }: DayBounds): string => {
    const date = dateOfLabel(label);
    return date === null ? label : shortDate(date);
  };
  return first.label === last.label ? name(first) : `${name(first)} – ${name(last)}`;
}
