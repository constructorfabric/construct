import { Alert, Button, Card, CardContent, Heading, Spinner } from '@gears-frontx/ui-kit';
import { Circle, CircleCheck } from 'lucide-react';
import type { ReactNode } from 'react';
import { isRunning, setupDays } from '../dashboard-setup-protocol';
import type { SetupStatus } from '../dashboard-setup-protocol';
import { t } from '../translation';
import styles from './app.module.css';
import { ButtonRow } from './components/ButtonRow';
import { Markdown } from './markdown';
import { formatSize } from './setup-text';
import type { DashboardSetup, SetupView } from './useDashboardSetup';

type ReadyView = Extract<SetupView, { kind: 'ready' }>;

/** The steps of a run, in order. */
const steps = ['writing_files', 'reading_and_building'] as const;

/** Where the person goes to set a model: the first field of the model section. */
function goToModel(): void {
  const field = document.getElementById('model-base-url');
  field?.scrollIntoView({ block: 'center' });
  field?.focus();
}

/** One step: an icon and a word, so the colour is never the only sign. */
function StepRow({
  step,
  tone,
}: {
  readonly step: (typeof steps)[number];
  readonly tone: 'done' | 'running' | 'waiting';
}): ReactNode {
  return (
    <li className={styles.check} data-step={step} data-state={tone}>
      <span className={styles.checkIcon} aria-hidden="true">
        {/* The row's text says what the loader means; the kit's own status role would say it twice. */}
        {tone === 'running' && <Spinner compact role="presentation" aria-label={undefined} />}
        {tone === 'done' && <CircleCheck className={styles.checkWorks} />}
        {tone === 'waiting' && <Circle className={styles.checkSkipped} />}
      </span>
      <span className={styles.checkText}>
        <span className={styles.checkName}>{t(`DashboardSetup.Step.${step}`)}</span>
        {t('DashboardSetup.Separator')}
        <span className={tone === 'waiting' ? styles.checkSkipped : undefined}>
          {tone === 'done'
            ? t('DashboardSetup.StepDone')
            : tone === 'running'
              ? t('DashboardSetup.StepRunning')
              : t('DashboardSetup.StepWaiting')}
        </span>
      </span>
    </li>
  );
}

function Progress({
  status,
  cancelling,
  unreachable,
  onCancel,
}: {
  readonly status: SetupStatus;
  readonly cancelling: boolean;
  readonly unreachable: string | null;
  readonly onCancel: () => void;
}): ReactNode {
  const current = steps.indexOf(status.state as (typeof steps)[number]);
  return (
    <>
      {unreachable !== null && (
        <Alert id="setup-unreachable" variant="destructive" role="alert" className={styles.outcome}>
          {t('DashboardSetup.Unreachable', { reason: unreachable })}
        </Alert>
      )}
      <ul id="setup-steps" className={styles.checks} aria-label={t('DashboardSetup.Steps')}>
        {steps.map((step, index) => (
          <StepRow
            key={step}
            step={step}
            tone={index < current ? 'done' : index === current ? 'running' : 'waiting'}
          />
        ))}
      </ul>
      <p id="setup-sizes" className={styles.setupText} role="status">
        {t('DashboardSetup.Read', {
          sessions: status.sessions,
          size: formatSize(status.textBytes),
        })}{' '}
        {t('DashboardSetup.Exchanged', {
          sent: formatSize(status.sent),
          received: formatSize(status.received),
        })}
      </p>
      <ButtonRow>
        <Button
          id="setup-cancel"
          type="button"
          variant="secondary"
          disabled={cancelling}
          onClick={onCancel}
        >
          {cancelling ? t('DashboardSetup.Cancelling') : t('DashboardSetup.Cancel')}
        </Button>
      </ButtonRow>
    </>
  );
}

function Report({
  status,
  onOpenDashboard,
}: {
  readonly status: SetupStatus;
  readonly onOpenDashboard: () => void;
}): ReactNode {
  const report = status.report?.trim() ?? '';
  return (
    <>
      <Card id="setup-result" className={styles.card}>
        <CardContent className={styles.resultBody}>
          <p id="setup-done" role="status" className={styles.resultDone}>
            {t('DashboardSetup.Done')}
          </p>
          <Heading id="setup-report-title" typography="heading-2" render={<h3 />}>
            {t('DashboardSetup.ReportTitle')}
          </Heading>
          <div id="setup-report" className={styles.report}>
            {report === '' ? (
              <p className={styles.setupMuted}>{t('DashboardSetup.NoReport')}</p>
            ) : (
              <Markdown
                text={report}
                onOpenLink={(address) => void window.constructAgent.openLink(address)}
                imageLabel={t('Markdown.Image')}
              />
            )}
          </div>
        </CardContent>
      </Card>
      <ButtonRow>
        <Button id="setup-open-dashboard" type="button" variant="link" onClick={onOpenDashboard}>
          {t('DashboardSetup.OpenDashboard')}
        </Button>
      </ButtonRow>
    </>
  );
}

/** What stops a run before it starts: no model, or collection off. */
function blocker(modelSet: boolean, collecting: boolean): 'no-model' | 'collection-off' | null {
  if (!modelSet) {
    return 'no-model';
  }
  return collecting ? null : 'collection-off';
}

function Starter({
  view,
  modelSet,
  collecting,
  onStart,
}: {
  readonly view: ReadyView;
  readonly modelSet: boolean;
  readonly collecting: boolean;
  readonly onStart: () => void;
}): ReactNode {
  const { status } = view;
  const stopped = blocker(modelSet, collecting);
  const failed = status.state === 'failed' || view.refused !== null;
  const label =
    failed && stopped === null
      ? t('DashboardSetup.Retry')
      : status.state === 'done'
        ? t('DashboardSetup.Rebuild')
        : t('DashboardSetup.Enable');
  return (
    <>
      {stopped === 'no-model' && (
        <Alert id="setup-no-model" role="status" className={styles.outcome}>
          <span>{t('DashboardSetup.NoModel')}</span>{' '}
          <Button id="setup-go-to-model" type="button" variant="link" onClick={goToModel}>
            {t('DashboardSetup.GoToModel')}
          </Button>
        </Alert>
      )}
      {stopped === 'collection-off' && (
        <Alert id="setup-collection-off" role="status" className={styles.outcome}>
          {t('DashboardSetup.CollectionOff')}
        </Alert>
      )}
      <ButtonRow>
        <Button
          id="setup-start"
          type="button"
          loading={view.pending === 'start'}
          disabled={stopped !== null || view.pending !== null}
          aria-describedby={
            stopped === 'no-model'
              ? 'setup-no-model'
              : stopped === 'collection-off'
                ? 'setup-collection-off'
                : undefined
          }
          onClick={onStart}
        >
          {label}
        </Button>
      </ButtonRow>
    </>
  );
}

function Body({
  view,
  modelSet,
  collecting,
  onStart,
  onCancel,
  onOpenDashboard,
}: {
  readonly view: SetupView;
  readonly modelSet: boolean;
  readonly collecting: boolean;
  readonly onStart: () => void;
  readonly onCancel: () => void;
  readonly onOpenDashboard: () => void;
}): ReactNode {
  if (view.kind === 'unavailable') {
    return (
      <Alert id="setup-unavailable" variant="destructive">
        {t('DashboardSetup.Unavailable', { reason: view.reason })}
      </Alert>
    );
  }
  // The Settings page waits for the state of the setup together with its own settings, so the
  // section is drawn once the state is there.
  if (view.kind === 'loading') {
    return null;
  }
  return (
    <ReadyBody
      view={view}
      modelSet={modelSet}
      collecting={collecting}
      onStart={onStart}
      onCancel={onCancel}
      onOpenDashboard={onOpenDashboard}
    />
  );
}

function ReadyBody({
  view,
  modelSet,
  collecting,
  onStart,
  onCancel,
  onOpenDashboard,
}: {
  readonly view: ReadyView;
  readonly modelSet: boolean;
  readonly collecting: boolean;
  readonly onStart: () => void;
  readonly onCancel: () => void;
  readonly onOpenDashboard: () => void;
}): ReactNode {
  const { status } = view;
  if (isRunning(status.state)) {
    return (
      <Progress
        status={status}
        cancelling={view.pending === 'cancel'}
        unreachable={view.unreachable}
        onCancel={onCancel}
      />
    );
  }
  const reason = view.refused ?? status.error ?? '';
  return (
    <>
      {status.state === 'done' && <Report status={status} onOpenDashboard={onOpenDashboard} />}
      {status.state === 'failed' && view.refused === null && (
        <Alert id="setup-failed" variant="destructive" role="alert" className={styles.outcome}>
          {t('DashboardSetup.Failed', { reason })}
        </Alert>
      )}
      {view.refused !== null && (
        <Alert id="setup-failed" variant="destructive" role="alert" className={styles.outcome}>
          {t('DashboardSetup.Refused', { reason })}
        </Alert>
      )}
      {status.state === 'cancelled' && view.refused === null && (
        <Alert id="setup-cancelled" role="status" className={styles.outcome}>
          {t('DashboardSetup.Cancelled')}
        </Alert>
      )}
      <Hint view={view} />
      <Starter view={view} modelSet={modelSet} collecting={collecting} onStart={onStart} />
    </>
  );
}

/** How much there is to read, before a run starts. */
function Hint({ view }: { readonly view: ReadyView }): ReactNode {
  const { sessionsInPeriod, status } = view;
  if (sessionsInPeriod === null && status.textBytes === 0) {
    return null;
  }
  return (
    <p id="setup-hint" className={`${styles.setupText} ${styles.setupMuted}`}>
      {sessionsInPeriod !== null &&
        t('DashboardSetup.Hint', { days: setupDays, sessions: sessionsInPeriod })}
      {sessionsInPeriod !== null && status.textBytes > 0 ? ' ' : ''}
      {status.textBytes > 0 && t('DashboardSetup.HintText', { size: formatSize(status.textBytes) })}
    </p>
  );
}

/** The Dashboard section of the Settings page: the button that sets the dashboard up, and the run. */
export function DashboardSetupSection({
  setup: { view, start, cancel },
  modelSet,
  collecting,
  onOpenDashboard,
}: {
  readonly setup: DashboardSetup;
  readonly modelSet: boolean;
  readonly collecting: boolean;
  readonly onOpenDashboard: () => void;
}): ReactNode {
  return (
    <section aria-labelledby="setup-section-title">
      <Heading id="setup-section-title" typography="heading-1" render={<h2 />}>
        {t('DashboardSetup.Title')}
      </Heading>
      <p className={styles.setupText}>{t('DashboardSetup.Intro')}</p>
      <Body
        view={view}
        modelSet={modelSet}
        collecting={collecting}
        onStart={start}
        onCancel={cancel}
        onOpenDashboard={onOpenDashboard}
      />
    </section>
  );
}
