import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  DashboardDocument,
  DashboardResult,
  DocumentProblem,
  ProjectChange,
  SummaryResult,
  Widget,
} from '../dashboard-protocol';
import { dayBounds, dayLabel, periodAnchor, periodDays } from './dashboard-period';
import { lostAgent } from './lost-agent';

/** How long the page waits before it asks again while the agent reads the history. */
const retryMs = 2000;

/** How often the page asks the agent whether the dashboard file has changed. */
const documentPollMs = 2000;

/**
 * How often the page counts again for the document and the day it already shows, so that work
 * done meanwhile appears without a click. Each count reads the whole history in the agent, so it
 * is not much more often than a person looks.
 */
const refreshMs = 15_000;

type Numbers = Extract<DashboardResult, { ok: true }>;
type Summary = Extract<SummaryResult, { ok: true }>;

/** The own and the unattended minutes of each day of a chart that shows both. */
export interface Split {
  readonly label: string;
  readonly own: number;
  readonly unattended: number;
}

/** What one widget has to draw, or why it cannot. */
export type WidgetNumbers =
  | { readonly ok: true; readonly numbers: Numbers; readonly split: readonly Split[] | null }
  | { readonly ok: true; readonly summary: Summary }
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

async function widgetNumbers(
  document: DashboardDocument,
  widget: Widget,
  anchor: Date,
): Promise<WidgetNumbers> {
  if (widget.kind === 'day_summary') {
    const summary = await window.constructAgent.getDashboardSummary({
      day: dayBounds(anchor),
      widget: widget.id,
    });
    return summary.ok ? { ok: true, summary } : summary;
  }
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

export interface DashboardControls {
  readonly state: DashboardState;
  /**
   * The document the page has read, also while the numbers are being counted: the toolbar stands on
   * it, so the arrows and the date do not vanish at every step.
   */
  readonly document: DashboardDocument | null;
  /**
   * Writes the changes to the document, one at a time and in the order asked, and shows the
   * document the agent answers. A change the agent refuses stops the rest of its list.
   */
  readonly change: (changes: readonly ProjectChange[]) => Promise<boolean>;
  /** Why the last change was not made, until the next change or `dismissFailure`. */
  readonly failure: string | null;
  readonly dismissFailure: () => void;
}

/**
 * The dashboard document with the numbers of each of its widgets, counted from the day the page
 * stands on (`day`; see `periodAnchor`), which stands for today. The agent watches the file, so
 * the page asks again every few seconds and reads the numbers again when the document, or the day,
 * changes, and every `refreshMs` besides.
 */
export function useDashboard(day: Date): DashboardControls {
  const [file, setFile] = useState<File | { readonly error: string } | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  // What the page last took from the agent.
  const last = useRef('');
  // Goes up when a change starts and again when it ends: a look that began before either would
  // bring back the document as it was, so its answer is dropped.
  const generation = useRef(0);
  // Changes are written one at a time: two at once could have their answers cross.
  const queue = useRef<Promise<boolean>>(Promise.resolve(true));
  const write = useCallback(async (list: readonly ProjectChange[]): Promise<boolean> => {
    generation.current += 1;
    setFailure(null);
    try {
      for (const one of list) {
        const result = await window.constructAgent.changeDashboardProject(one);
        if (!result.ok) {
          setFailure(result.error);
          return false;
        }
        last.current = JSON.stringify({ document: result.document, problem: result.problem });
        setFile({ document: result.document, problem: result.problem });
      }
      return true;
    } catch (error) {
      setFailure(lostAgent(error));
      return false;
    } finally {
      generation.current += 1;
    }
  }, []);
  const change = useCallback(
    (list: readonly ProjectChange[]): Promise<boolean> => {
      queue.current = queue.current.then(() => write(list));
      return queue.current;
    },
    [write],
  );
  const dismissFailure = useCallback(() => {
    setFailure(null);
  }, []);
  useEffect(() => {
    let cancelled = false;
    let hasDocument = false;
    const accept = (next: File | { readonly error: string }): void => {
      const text = JSON.stringify(next);
      if (!cancelled && text !== last.current) {
        last.current = text;
        hasDocument = 'document' in next && next.document !== null;
        setFile(next);
      }
    };
    const look = (): void => {
      const began = generation.current;
      window.constructAgent.getDashboardDocument().then(
        (result) => {
          if (began !== generation.current) {
            return;
          }
          // A failed look keeps the last good document, which the next look may confirm.
          if (result.ok) {
            accept({ document: result.document, problem: result.problem });
          } else if (!hasDocument) {
            accept({ error: result.error });
          }
        },
        (error: unknown) => {
          if (began === generation.current && !hasDocument) {
            accept({ error: lostAgent(error) });
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
  const anchor = periodAnchor(document, day);
  // The problem of the file is not a reason to count again: the document is the same.
  const key = `${dayLabel(anchor)}|${JSON.stringify(document)}`;
  // The same document and day but for the hidden projects and the names: only the numbers change.
  const shape = `${dayLabel(anchor)}|${JSON.stringify(
    document === null ? null : { ...document, hiddenProjects: [], projectNames: {} },
  )}`;
  const [loaded, setLoaded] = useState<{
    shape: string;
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
    // What the page shows now, so that a count that came out the same changes nothing.
    let shown = '';
    const read = (): void => {
      Promise.all([
        Promise.all(
          document.widgets.map(
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
          timer = setTimeout(read, refreshMs);
          const text = JSON.stringify([entries, settings]);
          if (text === shown) {
            return;
          }
          shown = text;
          // Without the settings the page cannot say whether collection is on.
          setLoaded({
            shape,
            numbers: new Map(entries),
            collecting: settings.ok && settings.collection.enabled,
            failure: settings.ok ? null : settings.error,
          });
        },
        (error: unknown) => {
          if (!cancelled) {
            timer = setTimeout(read, refreshMs);
            // A failed recount keeps the numbers on show, as a failed look at the file does.
            if (shown !== '') {
              return;
            }
            setLoaded({
              shape,
              numbers: new Map(),
              collecting: true,
              failure: lostAgent(error),
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
    // `key` stands for the document and the day; the same ones are counted again on a timer.
  }, [key]);

  const state = ((): DashboardState => {
    if (file === null) {
      return { kind: 'reading' };
    }
    if ('error' in file) {
      return { kind: 'failed', reason: file.error };
    }
    if (document === null) {
      return {
        kind: 'ready',
        document,
        problem: file.problem,
        numbers: new Map(),
        collecting: true,
      };
    }
    // Numbers of the same page, counted before a project was hidden or renamed, stay on show
    // until the new ones come: the rows do not vanish under the user's hands.
    if (loaded?.shape !== shape) {
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
  })();
  return { state, document, change, failure, dismissFailure };
}
