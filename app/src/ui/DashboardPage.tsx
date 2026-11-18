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
import { useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import type { DashboardDocument, DocumentProblem } from '../dashboard-protocol';
import { t } from '../translation';
import styles from './app.module.css';
import { DashboardGrid } from './DashboardGrid';
import { DashboardWidget } from './DashboardWidgets';
import { addDays, startOfDay } from 'date-fns';
import { drawnWidgets, useDashboard } from './useDashboard';
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

/** Whether nothing was counted for any of the widgets that show numbers. */
function nothingCounted(
  document: DashboardDocument,
  numbers: ReadonlyMap<string, WidgetNumbers>,
): boolean {
  const widgets = drawnWidgets(document);
  return (
    widgets.length > 0 &&
    widgets.every((widget) => {
      const counted = numbers.get(widget.id);
      return (
        counted?.ok === true &&
        counted.numbers.sessionCount === 0 &&
        counted.numbers.totalMinutes === 0
      );
    })
  );
}

function Widgets({
  document,
  numbers,
}: {
  readonly document: DashboardDocument;
  readonly numbers: ReadonlyMap<string, WidgetNumbers>;
}): ReactNode {
  const arrangeable = useArrangeable();
  // The same list for the same document: the grid starts again from the document's places
  // when the list changes, and so must not get a new one at every render.
  const widgets = useMemo(() => drawnWidgets(document), [document]);
  return (
    <DashboardGrid
      widgets={widgets}
      arrangeable={arrangeable}
      render={(widget) => (
        <DashboardWidget document={document} widget={widget} numbers={numbers.get(widget.id)} />
      )}
    />
  );
}

function Body({
  state,
  onOpenSettings,
}: {
  readonly state: DashboardState;
  readonly onOpenSettings: () => void;
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
  if (drawnWidgets(document).length === 0) {
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
      <Widgets document={document} numbers={numbers} />
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
  const state = useDashboard(day);
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
      <Body state={state} onOpenSettings={onOpenSettings} />
    </>
  );
}
