import type { DayBounds, Source, Whose, Widget } from '../dashboard-protocol';
import type { SessionsSource } from './useSessions';

/**
 * What a click on `group` of `widget` asks the agent for. A table counts its groups as its own
 * widget does. A chart has no grouping of its own, so its bars are days: the days are counted
 * with the sessions and the filter the widget has; the time on screen has no sessions.
 */
export function drillSource(
  widget: Widget,
  days: readonly DayBounds[],
  sessions: Whose,
  group: string,
  source: Source = 'ai_sessions',
): SessionsSource {
  if (widget.kind !== 'bars_per_day') {
    return { days, group, widget: widget.id };
  }
  if (source === 'on_screen') {
    return {
      days,
      group,
      source,
      groupBy: 'day',
      ...(widget.filter === undefined ? {} : { filter: widget.filter }),
    };
  }
  return {
    days,
    group,
    sessions,
    groupBy: 'day',
    ...(widget.filter === undefined ? {} : { filter: widget.filter }),
  };
}
