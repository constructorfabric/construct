import { useEffect, useState } from 'react';
import type {
  DashboardDocument,
  DashboardResult,
  DocumentProblem,
  Widget,
} from '../dashboard-protocol';
import { dayLabel, periodDays } from './dashboard-period';

/** How long the page waits before it asks again while the agent reads the history. */
const retryMs = 2000;

/** How often the page asks the agent whether the dashboard file has changed. */
const documentPollMs = 2000;

type Numbers = Extract<DashboardResult, { ok: true }>;

/** The own and the unattended minutes of each day of a chart that shows both. */
export interface Split {
  readonly label: string;
  readonly own: number;
  readonly unattended: number;
}

/** What one widget has to draw, or why it cannot. */
export type WidgetNumbers =
  | { readonly ok: true; readonly numbers: Numbers; readonly split: readonly Split[] | null }
  | { readonly ok: false; readonly error: string; readonly reading?: true };

/** What the page knows of the dashboard it shows. */
export type DashboardState =
  | { readonly kind: 'reading' }
  | { readonly kind: 'failed'; readonly reason: string }
  | {
      readonly kind: 'ready';
      /** The last good document; none when the file has never passed. */
      readonly document: DashboardDocument | null;
      /** Why the file as it is now is not used. */
      readonly problem: DocumentProblem | null;
      /** The numbers of each widget, by the widget's id. */
      readonly numbers: ReadonlyMap<string, WidgetNumbers>;
      /** Whether the agent collects the user's work; `true` when it did not say. */
      readonly collecting: boolean;
    };

type File = {
  readonly document: DashboardDocument | null;
  readonly problem: DocumentProblem | null;
};

/** The widgets that show numbers; the summary of the day has no drawing here. */
export function drawnWidgets(document: DashboardDocument): Widget[] {
  return document.widgets.filter((widget) => widget.kind !== 'day_summary');
}

async function widgetNumbers(
  document: DashboardDocument,
  widget: Widget,
  anchor: Date,
): Promise<WidgetNumbers> {
  const days = periodDays(widget.period ?? document.period, anchor);
  const all = await window.constructAgent.getDashboardNumbers({ days, widget: widget.id });
  if (!all.ok) {
    return all;
  }
  if (widget.kind !== 'bars_per_day' || (widget.sessions ?? document.sessions) !== 'all') {
    return { ok: true, numbers: all, split: null };
  }
  // The user's own time is counted on its own; the rest of the time of both is the unattended.
  const own = await window.constructAgent.getDashboardNumbers({
    days,
    sessions: 'own',
    ...(widget.filter === undefined ? {} : { filter: widget.filter }),
  });
  if (!own.ok) {
    return own;
  }
  const split = all.days.map((day, index) => {
    const mine = Math.min(own.days[index]?.minutes ?? 0, day.minutes);
    return { label: day.label, own: mine, unattended: day.minutes - mine };
  });
  return { ok: true, numbers: all, split };
}

/**
 * The dashboard document with the numbers of each of its widgets, counted from `anchor`, which
 * stands for today. The agent watches the file, so the page asks again every few seconds and
 * reads the numbers again when the document, or the day, changes.
 */
export function useDashboard(anchor: Date): DashboardState {
  const [file, setFile] = useState<File | { readonly error: string } | null>(null);
  useEffect(() => {
    let cancelled = false;
    let last = '';
    let hasDocument = false;
    const accept = (next: File | { readonly error: string }): void => {
      const text = JSON.stringify(next);
      if (!cancelled && text !== last) {
        last = text;
        hasDocument = 'document' in next && next.document !== null;
        setFile(next);
      }
    };
    const look = (): void => {
      window.constructAgent.getDashboardDocument().then(
        (result) => {
          // A failed look keeps the last good document, which the next look may confirm.
          if (result.ok) {
            accept({ document: result.document, problem: result.problem });
          } else if (!hasDocument) {
            accept({ error: result.error });
          }
        },
        (error: unknown) => {
          if (!hasDocument) {
            accept({ error: error instanceof Error ? error.message : 'the window lost the agent' });
          }
        },
      );
    };
    look();
    const timer = setInterval(look, documentPollMs);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);

  const document = file !== null && 'document' in file ? file.document : null;
  // The problem of the file is not a reason to count again: the document is the same.
  const key = `${dayLabel(anchor)}|${JSON.stringify(document)}`;
  const [loaded, setLoaded] = useState<{
    key: string;
    numbers: ReadonlyMap<string, WidgetNumbers>;
    collecting: boolean;
    failure: string | null;
  } | null>(null);
  useEffect(() => {
    if (document === null) {
      return;
    }
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const read = (): void => {
      Promise.all([
        Promise.all(
          drawnWidgets(document).map(
            async (widget) => [widget.id, await widgetNumbers(document, widget, anchor)] as const,
          ),
        ),
        window.constructAgent.getSettings(),
      ]).then(
        ([entries, settings]) => {
          if (cancelled) {
            return;
          }
          // The agent reads the history first: the page says so, and asks again.
          if (entries.some(([, counted]) => !counted.ok && counted.reading === true)) {
            timer = setTimeout(read, retryMs);
            return;
          }
          // Without the settings the page cannot say whether collection is on.
          setLoaded({
            key,
            numbers: new Map(entries),
            collecting: settings.ok && settings.collection.enabled,
            failure: settings.ok ? null : settings.error,
          });
        },
        (error: unknown) => {
          if (!cancelled) {
            setLoaded({
              key,
              numbers: new Map(),
              collecting: true,
              failure: error instanceof Error ? error.message : 'the window lost the agent',
            });
          }
        },
      );
    };
    read();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
    // `key` stands for the document and the day: the same ones are not read again.
  }, [key]);

  if (file === null) {
    return { kind: 'reading' };
  }
  if ('error' in file) {
    return { kind: 'failed', reason: file.error };
  }
  if (document === null) {
    return { kind: 'ready', document, problem: file.problem, numbers: new Map(), collecting: true };
  }
  if (loaded?.key !== key) {
    return { kind: 'reading' };
  }
  if (loaded.failure !== null) {
    return { kind: 'failed', reason: loaded.failure };
  }
  return {
    kind: 'ready',
    document,
    problem: file.problem,
    numbers: loaded.numbers,
    collecting: loaded.collecting,
  };
}
