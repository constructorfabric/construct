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
import { useEffect, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import type { AgentStatus } from '../agent-status';
import type {
  DashboardDocument,
  DocumentProblem,
  ProjectChange,
  Source,
} from '../dashboard-protocol';
import { t } from '../translation';
import styles from './app.module.css';
import { DelayedLoader } from './components/DelayedLoader';
import { cancelRowsBack, expectRowsBack } from './focus-after-show';
import { DashboardGrid } from './DashboardGrid';
import { DashboardWidget } from './DashboardWidgets';
import { addDays, startOfDay } from 'date-fns';
import { countsDay, mondayOf, pageLabel, periodAnchor } from './dashboard-period';
import { missingGrants, sourceWidgets, sourcesUsed, gridWidgets } from './dashboard-source';
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

/** Whether nothing was counted for any of the widgets. */
function nothingCounted(
  widgets: DashboardDocument['widgets'],
  numbers: ReadonlyMap<string, WidgetNumbers>,
): boolean {
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
  source,
  numbers,
  day,
  onChange,
}: {
  readonly document: DashboardDocument;
  /** The source whose widgets the page shows. */
  readonly source: Source;
  readonly numbers: ReadonlyMap<string, WidgetNumbers>;
  readonly day: Date;
  readonly onChange: (changes: readonly ProjectChange[]) => void;
}): ReactNode {
  const arrangeable = useArrangeable();
  // The same list for the same document: the grid starts again from the document's places
  // when the list changes, and so must not get a new one at every render.
  const widgets = useMemo(() => gridWidgets(document, source), [document, source]);
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

/**
 * What the page shows under its toolbar: the failure, else the delayed loader of the first wait
 * (it stays on show while the agent reads the history), else the page. A page that has been shown
 * or has failed and is counted again gives way to the loader at once.
 */
function Body({
  state,
  day,
  chosen,
  onChoose,
  status,
  onOpenSettings,
  onChange,
  onShow,
}: {
  readonly state: DashboardState;
  readonly day: Date;
  readonly chosen: Source;
  readonly onChoose: (source: Source) => void;
  readonly status: AgentStatus | null;
  readonly onOpenSettings: () => void;
  readonly onChange: (changes: readonly ProjectChange[]) => void;
  readonly onShow: ShowHidden;
}): ReactNode {
  // Whether the page has answered once, with the page or with a failure. It is set while
  // rendering, as React allows for state of the component's own, so that the render that sees
  // the change already knows it.
  const [answered, setAnswered] = useState(false);
  if (state.kind !== 'reading' && !answered) {
    setAnswered(true);
  }
  if (state.kind === 'failed') {
    return (
      <Alert id="dashboard-failed" variant="destructive">
        {t('DashboardPage.Unavailable', { reason: state.reason })}
      </Alert>
    );
  }
  const reading = t('DashboardPage.Reading');
  if (state.kind === 'reading' && answered) {
    return <Spinner id="dashboard-reading" label={reading} />;
  }
  return (
    <DelayedLoader waiting={state.kind === 'reading'} id="dashboard-reading" label={reading}>
      {state.kind === 'ready' && (
        <ReadyBody
          state={state}
          day={day}
          chosen={chosen}
          onChoose={onChoose}
          status={status}
          onOpenSettings={onOpenSettings}
          onChange={onChange}
          onShow={onShow}
        />
      )}
    </DelayedLoader>
  );
}

function ReadyBody({
  state,
  day,
  chosen,
  onChoose,
  status,
  onOpenSettings,
  onChange,
  onShow,
}: {
  readonly state: Extract<DashboardState, { readonly kind: 'ready' }>;
  readonly day: Date;
  readonly chosen: Source;
  readonly onChoose: (source: Source) => void;
  readonly status: AgentStatus | null;
  readonly onOpenSettings: () => void;
  readonly onChange: (changes: readonly ProjectChange[]) => void;
  readonly onShow: ShowHidden;
}): ReactNode {
  const { document, problem, numbers, collecting, screen: screenState } = state;
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
  const sources = sourcesUsed(document);
  const source = sources.includes(chosen) ? chosen : (sources[0] ?? 'ai_sessions');
  const screen = source === 'on_screen';
  // What is counted on the page is what its source counts: the other source is not part of it.
  const widgets = sourceWidgets(document, source);
  const shown = new Map(
    widgets.flatMap((widget) => {
      const counted = numbers.get(widget.id);
      return counted === undefined ? [] : [[widget.id, counted] as const];
    }),
  );
  const hidden = hiddenCount(shown);
  const grants = status?.state === 'running' ? status.grants : undefined;
  // The time on screen is off when the person has not turned it on; a permission that is missing
  // is named so that they know what to give.
  const off = screen ? screenState !== 'on' : !collecting;
  const missing = screen && screenState === 'on' ? missingGrants(grants) : [];
  const names = missing.map((name) => t(`ScreenGrants.Name.${name}`)).join(', ');
  // A computer that cannot collect the time on screen has no switch in Settings to send the person to.
  const unsupported = screen && screenState === 'unsupported';
  const offKey = unsupported ? 'Screen.Unsupported' : screen ? 'Screen.Off' : 'Off';
  const settingsButton = (variant: 'link' | 'outline'): ReactNode => (
    <Button
      id="dashboard-open-settings"
      variant={variant}
      size={variant === 'link' ? undefined : 'sm'}
      onClick={onOpenSettings}
    >
      {t('DashboardPage.OpenSettings')}
    </Button>
  );
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
  const choice = sources.length > 1 && (
    <div
      className={styles.toolbarGroup}
      role="group"
      id="dashboard-source"
      aria-label={t('DashboardPage.Source.Group')}
    >
      {sources.map((one) => (
        <Button
          key={one}
          id={`dashboard-source-${one}`}
          variant={one === source ? 'default' : 'outline'}
          size="sm"
          aria-pressed={one === source}
          onClick={() => {
            onChoose(one);
          }}
        >
          {t(`DashboardPage.Source.${one}`)}
        </Button>
      ))}
    </div>
  );
  if (nothingCounted(widgets, shown) && hidden > 0) {
    return (
      <>
        {problemNote}
        {choice}
        <AllHidden count={hidden} document={document} onShow={onShow} />
      </>
    );
  }
  if (nothingCounted(widgets, shown)) {
    return (
      <>
        {problemNote}
        {choice}
        {off ? (
          <Empty id={screen ? 'dashboard-screen-off' : 'dashboard-off'}>
            <EmptyHeader>
              <EmptyTitle render={<h2 />}>{t(`DashboardPage.${offKey}.Title`)}</EmptyTitle>
              <EmptyDescription>{t(`DashboardPage.${offKey}.Text`)}</EmptyDescription>
            </EmptyHeader>
            {!unsupported && <EmptyActions>{settingsButton('link')}</EmptyActions>}
          </Empty>
        ) : missing.length > 0 ? (
          <Empty id="dashboard-screen-permission">
            <EmptyHeader>
              <EmptyTitle render={<h2 />}>{t('DashboardPage.Screen.Permission.Title')}</EmptyTitle>
              <EmptyDescription>
                {t('DashboardPage.Screen.Permission.Text', { names })}
              </EmptyDescription>
            </EmptyHeader>
            <EmptyActions>{settingsButton('link')}</EmptyActions>
          </Empty>
        ) : (
          <Empty id={screen ? 'dashboard-screen-empty' : 'dashboard-empty'}>
            <EmptyHeader>
              <EmptyTitle render={<h2 />}>
                {t(screen ? 'DashboardPage.Screen.Empty.Title' : 'DashboardPage.Empty.Title')}
              </EmptyTitle>
              <EmptyDescription>
                {t(screen ? 'DashboardPage.Screen.Empty.Text' : 'DashboardPage.Empty.Text')}
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        )}
      </>
    );
  }
  return (
    <>
      {problemNote}
      {choice}
      {off && (
        <Alert id={screen ? 'dashboard-screen-off-note' : 'dashboard-off-note'}>
          <span className={styles.alertContent}>
            {t(`DashboardPage.${offKey}.Note`)}
            {!unsupported && settingsButton('outline')}
          </span>
        </Alert>
      )}
      {missing.length > 0 && (
        <Alert id="dashboard-screen-permission-note">
          <span className={styles.alertContent}>
            {t('DashboardPage.Screen.Permission.Note', { names })}
            {settingsButton('outline')}
          </span>
        </Alert>
      )}
      {hidden > 0 && (
        <HiddenNote count={hidden} document={document} focusRows={false} onShow={onShow} />
      )}
      <Widgets
        document={document}
        source={source}
        numbers={numbers}
        day={day}
        onChange={onChange}
      />
    </>
  );
}

/** The dashboard: the widgets of the dashboard document, counted as of the day the page shows. */
export function DashboardPage({
  status,
  onOpenSettings,
}: {
  /** The agent's latest status, which the app polls: the permissions are read from it. */
  readonly status: AgentStatus | null;
  readonly onOpenSettings: () => void;
}): ReactNode {
  const [day, setDay] = useState(() => startOfDay(new Date()));
  // The source of the time the page shows, when its document has widgets of both.
  const [chosen, setChosen] = useState<Source>('ai_sessions');
  const { state, document, change, failure, dismissFailure } = useDashboard(day);
  // The arrows step by the shortest period a widget counts: a day when one counts a day, else a
  // week, and then the week arrows are the only ones.
  const byDay = countsDay(document);
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
          {byDay && (
            <Button
              id="dashboard-previous-day"
              variant="outline"
              size="sm"
              aria-label={t('DashboardPage.Previous.Day')}
              icon={<ChevronLeft />}
              onClick={() => move(-1)}
            />
          )}
          <DatePicker
            id="dashboard-date"
            selected={day}
            onSelect={(picked) => {
              if (picked !== undefined) {
                // The date of a page that counts weeks only stands for its week.
                setDay(byDay ? startOfDay(picked) : mondayOf(startOfDay(picked)));
              }
            }}
            placeholder={t('DashboardPage.PickDate')}
          />
          {byDay && (
            <Button
              id="dashboard-next-day"
              variant="outline"
              size="sm"
              aria-label={t('DashboardPage.Next.Day')}
              icon={<ChevronRight />}
              onClick={() => move(1)}
            />
          )}
          <Button
            id="dashboard-next-week"
            variant="outline"
            size="sm"
            aria-label={t('DashboardPage.Next.Week')}
            icon={<ChevronsRight />}
            onClick={() => move(7)}
          />
        </div>
        {document !== null && (
          <p id="dashboard-period" className={styles.toolbarPeriod}>
            {pageLabel(document, day)}
          </p>
        )}
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
        day={periodAnchor(document, day)}
        chosen={chosen}
        onChoose={setChosen}
        status={status}
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
