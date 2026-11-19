import {
  Alert,
  Button,
  DatePicker,
  Empty,
  EmptyActions,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
  Heading,
  Spinner,
} from '@gears-frontx/ui-kit';
import { ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight } from 'lucide-react';
import { useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import type { DashboardDocument, DocumentProblem, ProjectChange } from '../dashboard-protocol';
import { t } from '../translation';
import styles from './app.module.css';
import { cancelRowsBack, expectRowsBack } from './focus-after-show';
import { DashboardGrid } from './DashboardGrid';
import { DashboardWidget } from './DashboardWidgets';
import { addDays, startOfDay } from 'date-fns';
import { useDashboard } from './useDashboard';
import type { DashboardState, WidgetNumbers } from './useDashboard';

/** Dragging and resizing have no control yet; the address of the page can turn them on. */
function useArrangeable(): boolean {
  return new URLSearchParams(window.location.search).has('arrange');
}

function ProblemNote({
  problem,
  shownLast,
}: {
  readonly problem: DocumentProblem;
  readonly shownLast: boolean;
}): ReactNode {
  return (
    <Alert id="dashboard-problem" variant="destructive">
      <span className={styles.alertContent}>
        <span>
          {problem.place === ''
            ? t('DashboardPage.Problem', { message: problem.message })
            : t('DashboardPage.Problem.At', { place: problem.place, message: problem.message })}
        </span>
        {shownLast && <span>{t('DashboardPage.Problem.LastGood')}</span>}
      </span>
    </Alert>
  );
}

/** Whether nothing was counted for any widget. */
function nothingCounted(
  document: DashboardDocument,
  numbers: ReadonlyMap<string, WidgetNumbers>,
): boolean {
  const { widgets } = document;
  return (
    widgets.length > 0 &&
    widgets.every((widget) => {
      const counted = numbers.get(widget.id);
      if (counted?.ok !== true) {
        return false;
      }
      // A summary with a line is time; one without is none.
      return 'summary' in counted
        ? counted.summary.lines.length === 0
        : counted.numbers.sessionCount === 0 && counted.numbers.totalMinutes === 0;
    })
  );
}

/** The most hidden projects any widget leaves out of its own period. */
function hiddenCount(numbers: ReadonlyMap<string, WidgetNumbers>): number {
  return Math.max(
    0,
    ...[...numbers.values()].map((counted) =>
      counted.ok && 'numbers' in counted ? counted.numbers.hiddenProjects : 0,
    ),
  );
}

/** Shows the hidden projects again; `focusRows` says that the table is drawn anew when they are. */
type ShowHidden = (projects: readonly string[], focusRows: boolean) => void;

function HiddenNote({
  count,
  document,
  focusRows,
  onShow,
}: {
  readonly count: number;
  readonly document: DashboardDocument;
  readonly focusRows: boolean;
  readonly onShow: ShowHidden;
}): ReactNode {
  return (
    <Alert id="dashboard-hidden-note">
      <span className={styles.alertContent}>
        {t(count === 1 ? 'DashboardPage.Hidden.One' : 'DashboardPage.Hidden.Many', { count })}
        <Button
          id="dashboard-show-hidden"
          variant="outline"
          size="sm"
          onClick={() => {
            onShow(document.hiddenProjects, focusRows);
          }}
        >
          {t('DashboardPage.Hidden.Show')}
        </Button>
      </span>
    </Alert>
  );
}

/** Every project with time is hidden: the note with its button stays, and takes the focus. */
function AllHidden({
  count,
  document,
  onShow,
}: {
  readonly count: number;
  readonly document: DashboardDocument;
  readonly onShow: ShowHidden;
}): ReactNode {
  useEffect(() => {
    // The button that had the focus was in the table that is gone; the focus would be nowhere.
    if (window.document.activeElement === window.document.body) {
      window.document.getElementById('dashboard-show-hidden')?.focus();
    }
  }, []);
  return (
    <>
      <HiddenNote count={count} document={document} focusRows onShow={onShow} />
      <Empty id="dashboard-all-hidden">
        <EmptyHeader>
          <EmptyTitle render={<h2 />}>{t('DashboardPage.AllHidden.Title')}</EmptyTitle>
          <EmptyDescription>{t('DashboardPage.AllHidden.Text')}</EmptyDescription>
        </EmptyHeader>
      </Empty>
    </>
  );
}

function Widgets({
  document,
  numbers,
  day,
  onChange,
}: {
  readonly document: DashboardDocument;
  readonly numbers: ReadonlyMap<string, WidgetNumbers>;
  readonly day: Date;
  readonly onChange: (changes: readonly ProjectChange[]) => void;
}): ReactNode {
  const arrangeable = useArrangeable();
  // The same list for the same document: the grid starts again from the document's places
  // when the list changes, and so must not get a new one at every render.
  const widgets = document.widgets;
  return (
    <DashboardGrid
      widgets={widgets}
      arrangeable={arrangeable}
      render={(widget) => (
        <DashboardWidget
          document={document}
          widget={widget}
          numbers={numbers.get(widget.id)}
          day={day}
          onChange={onChange}
        />
      )}
    />
  );
}

function Body({
  state,
  day,
  onOpenSettings,
  onChange,
  onShow,
}: {
  readonly state: DashboardState;
  readonly day: Date;
  readonly onOpenSettings: () => void;
  readonly onChange: (changes: readonly ProjectChange[]) => void;
  readonly onShow: ShowHidden;
}): ReactNode {
  if (state.kind === 'reading') {
    return <Spinner id="dashboard-reading" label={t('DashboardPage.Reading')} />;
  }
  if (state.kind === 'failed') {
    return (
      <Alert id="dashboard-failed" variant="destructive">
        {t('DashboardPage.Unavailable', { reason: state.reason })}
      </Alert>
    );
  }
  const { document, problem, numbers, collecting } = state;
  if (document === null) {
    return problem === null ? (
      <Empty id="dashboard-none">
        <EmptyHeader>
          <EmptyTitle render={<h2 />}>{t('DashboardPage.None.Title')}</EmptyTitle>
          <EmptyDescription>{t('DashboardPage.None.Text')}</EmptyDescription>
        </EmptyHeader>
      </Empty>
    ) : (
      <ProblemNote problem={problem} shownLast={false} />
    );
  }
  const problemNote = problem === null ? null : <ProblemNote problem={problem} shownLast />;
  const hidden = hiddenCount(numbers);
  if (document.widgets.length === 0) {
    return (
      <>
        {problemNote}
        <Empty id="dashboard-no-widgets">
          <EmptyHeader>
            <EmptyTitle render={<h2 />}>{t('DashboardPage.NoWidgets.Title')}</EmptyTitle>
            <EmptyDescription>{t('DashboardPage.NoWidgets.Text')}</EmptyDescription>
          </EmptyHeader>
        </Empty>
      </>
    );
  }
  if (nothingCounted(document, numbers) && hidden > 0) {
    return (
      <>
        {problemNote}
        <AllHidden count={hidden} document={document} onShow={onShow} />
      </>
    );
  }
  if (nothingCounted(document, numbers)) {
    return (
      <>
        {problemNote}
        {collecting ? (
          <Empty id="dashboard-empty">
            <EmptyHeader>
              <EmptyTitle render={<h2 />}>{t('DashboardPage.Empty.Title')}</EmptyTitle>
              <EmptyDescription>{t('DashboardPage.Empty.Text')}</EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <Empty id="dashboard-off">
            <EmptyHeader>
              <EmptyTitle render={<h2 />}>{t('DashboardPage.Off.Title')}</EmptyTitle>
              <EmptyDescription>{t('DashboardPage.Off.Text')}</EmptyDescription>
            </EmptyHeader>
            <EmptyActions>
              <Button id="dashboard-open-settings" variant="link" onClick={onOpenSettings}>
                {t('DashboardPage.OpenSettings')}
              </Button>
            </EmptyActions>
          </Empty>
        )}
      </>
    );
  }
  return (
    <>
      {problemNote}
      {!collecting && (
        <Alert id="dashboard-off-note">
          <span className={styles.alertContent}>
            {t('DashboardPage.Off.Note')}
            <Button
              id="dashboard-open-settings"
              variant="outline"
              size="sm"
              onClick={onOpenSettings}
            >
              {t('DashboardPage.OpenSettings')}
            </Button>
          </span>
        </Alert>
      )}
      {hidden > 0 && (
        <HiddenNote count={hidden} document={document} focusRows={false} onShow={onShow} />
      )}
      <Widgets document={document} numbers={numbers} day={day} onChange={onChange} />
    </>
  );
}

/** The dashboard: the widgets of the dashboard document, counted as of the day the page shows. */
export function DashboardPage({
  onOpenSettings,
}: {
  readonly onOpenSettings: () => void;
}): ReactNode {
  const [day, setDay] = useState(() => startOfDay(new Date()));
  const { state, change, failure, dismissFailure } = useDashboard(day);
  const move = (days: number): void => {
    setDay((current) => addDays(current, days));
  };

  return (
    <>
      <Heading typography="display" render={<h1 />}>
        {t('DashboardPage.Title')}
      </Heading>
      <div className={styles.toolbar}>
        <div
          className={styles.toolbarGroup}
          role="group"
          aria-label={t('DashboardPage.Navigation')}
        >
          <Button
            id="dashboard-previous-week"
            variant="outline"
            size="sm"
            aria-label={t('DashboardPage.Previous.Week')}
            icon={<ChevronsLeft />}
            onClick={() => move(-7)}
          />
          <Button
            id="dashboard-previous-day"
            variant="outline"
            size="sm"
            aria-label={t('DashboardPage.Previous.Day')}
            icon={<ChevronLeft />}
            onClick={() => move(-1)}
          />
          <DatePicker
            id="dashboard-date"
            selected={day}
            onSelect={(picked) => {
              if (picked !== undefined) {
                setDay(startOfDay(picked));
              }
            }}
            placeholder={t('DashboardPage.PickDate')}
          />
          <Button
            id="dashboard-next-day"
            variant="outline"
            size="sm"
            aria-label={t('DashboardPage.Next.Day')}
            icon={<ChevronRight />}
            onClick={() => move(1)}
          />
          <Button
            id="dashboard-next-week"
            variant="outline"
            size="sm"
            aria-label={t('DashboardPage.Next.Week')}
            icon={<ChevronsRight />}
            onClick={() => move(7)}
          />
        </div>
      </div>
      {failure !== null && (
        <Alert id="dashboard-change-failed" variant="destructive">
          <span className={styles.alertContent}>
            {t('DashboardPage.Change.Failed', { reason: failure })}
            <Button
              id="dashboard-change-dismiss"
              variant="outline"
              size="sm"
              onClick={dismissFailure}
            >
              {t('DashboardPage.Change.Dismiss')}
            </Button>
          </span>
        </Alert>
      )}
      <Body
        state={state}
        day={day}
        onOpenSettings={onOpenSettings}
        onChange={(changes) => void change(changes)}
        onShow={(projects, focusRows) => {
          // The first row's button takes the focus when the table is drawn anew, but not when the
          // change is refused, nor when the table stays and is only counted again.
          if (focusRows) {
            expectRowsBack();
          }
          void change(projects.map((project) => ({ action: 'show', project }))).then((made) => {
            if (!made) {
              cancelRowsBack();
            }
          });
        }}
      />
    </>
  );
}
