import {
  Badge,
  Card,
  CardAction,
  CardContent,
  CardHeader,
  CardTitle,
  Heading,
  Skeleton,
  StatusDot,
  Text,
  VisuallyHidden,
} from '@gears-frontx/ui-kit';
import { format } from 'date-fns';
import type { ReactNode } from 'react';
import type { AgentStatus, Connector, Part, ScreenCollection } from '../agent-status';
import { t } from '../translation';
import styles from './app.module.css';
import { useDelayedLoader } from './components/useDelayedLoader';
import { connectorName } from './connector-name';
import { connectorTones, screenTones, tones } from './state-tones';

const loginVariants = {
  registered: 'success',
  not_registered: 'secondary',
  unknown: 'secondary',
  not_supported: 'secondary',
} as const;

const variants = { running: 'success', stopped: 'danger', unknown: 'secondary' } as const;

const partVariants = { ready: 'success', starting: 'secondary', failed: 'danger' } as const;
const partTones = { ready: 'success', starting: 'neutral', failed: 'danger' } as const;

/** One part the agent runs: its name, its state and, when it is not ready, why. */
function PartCard({
  id,
  title,
  part,
}: {
  readonly id: string;
  readonly title: string;
  readonly part: Part;
}): ReactNode {
  return (
    <Card className={styles.card}>
      <CardHeader>
        <CardTitle render={<h2 />}>{title}</CardTitle>
        <CardAction className={styles.state}>
          <StatusDot tone={partTones[part.state]} />
          <Badge id={`${id}-state`} variant={partVariants[part.state]} data-state={part.state}>
            {t(`StatusPage.Part.${part.state}`)}
          </Badge>
        </CardAction>
      </CardHeader>
      {part.state !== 'ready' && part.reason !== undefined && (
        <CardContent>
          <Text id={`${id}-reason`} tone="muted" className={styles.detail}>
            {part.reason}
          </Text>
        </CardContent>
      )}
    </Card>
  );
}

const toneVariants = { success: 'success', neutral: 'secondary', danger: 'danger' } as const;
const connectorVariants = {
  collecting: toneVariants[connectorTones.collecting],
  scanning: toneVariants[connectorTones.scanning],
  off: toneVariants[connectorTones.off],
  failed: toneVariants[connectorTones.failed],
} as const;

/** How long ago something was stored, in words: just now, minutes, or the date. */
function whenText(at: number, now: number): string {
  const minutes = Math.floor((now - at) / 60_000);
  return minutes < 1
    ? t('StatusPage.Connector.JustNow')
    : minutes < 60
      ? t('StatusPage.Connector.MinutesAgo', { minutes })
      : format(at, 'd MMM yyyy, HH:mm');
}

/** When the last turn was stored, in words; an older agent that sends no time has none to show. */
function lastTurnText(lastTurnAt: number | undefined, now: number): string {
  return lastTurnAt === undefined
    ? t('StatusPage.Connector.NoTurn')
    : t('StatusPage.Connector.LastTurn', { when: whenText(lastTurnAt, now) });
}

/** Why the apps and websites collection is not collecting: the grants it lacks, or the agent's reason. */
function screenDetail(screen: ScreenCollection): string | undefined {
  if (screen.state === 'needs_permission' && screen.missing.length > 0) {
    return t('StatusPage.Screen.Missing', {
      names: screen.missing.map((name) => t(`ScreenGrants.Name.${name}`)).join(', '),
    });
  }
  return screen.reason;
}

/** The apps and websites collection: its state in words, why when it is not collecting, samples and last sample. */
function ScreenCard({
  screen,
  now,
}: {
  readonly screen: ScreenCollection;
  readonly now: number;
}): ReactNode {
  const detail = screenDetail(screen);
  return (
    <Card className={styles.card} id="screen">
      <CardHeader>
        <CardTitle render={<h3 />}>{t('StatusPage.Screen.Title')}</CardTitle>
        <CardAction className={styles.state}>
          <StatusDot tone={screenTones[screen.state]} />
          <Badge
            id="screen-state"
            variant={toneVariants[screenTones[screen.state]]}
            data-state={screen.state}
          >
            {t(`StatusPage.Screen.${screen.state}`)}
          </Badge>
        </CardAction>
      </CardHeader>
      <CardContent>
        {detail !== undefined && (
          <Text id="screen-reason" tone="muted" className={styles.detail}>
            {detail}
          </Text>
        )}
        <Text id="screen-samples" tone="muted" className={styles.detail}>
          {t('StatusPage.Screen.Samples', { count: screen.samplesToday })}
        </Text>
        <Text id="screen-last-sample" tone="muted" className={styles.detail}>
          {screen.lastSampleAt === undefined
            ? t('StatusPage.Screen.NoSample')
            : t('StatusPage.Screen.LastSample', { when: whenText(screen.lastSampleAt, now) })}
        </Text>
      </CardContent>
    </Card>
  );
}

/** One tool the agent collects from: its state in words, why when it failed, its count and last turn. */
function ConnectorCard({
  connector,
  now,
}: {
  readonly connector: Connector;
  readonly now: number;
}): ReactNode {
  const id = `connector-${connector.name}`;
  return (
    <Card className={styles.card} id={id}>
      <CardHeader>
        <CardTitle render={<h3 />}>{connectorName(connector.name)}</CardTitle>
        <CardAction className={styles.state}>
          <StatusDot tone={connectorTones[connector.state]} />
          <Badge
            id={`${id}-state`}
            variant={connectorVariants[connector.state]}
            data-state={connector.state}
          >
            {t(`StatusPage.Connector.${connector.state}`)}
          </Badge>
        </CardAction>
      </CardHeader>
      <CardContent>
        {connector.reason !== undefined && (
          <Text id={`${id}-reason`} tone="muted" className={styles.detail}>
            {connector.reason}
          </Text>
        )}
        <Text id={`${id}-turns`} tone="muted" className={styles.detail}>
          {t('StatusPage.Connector.Turns', { count: connector.turns })}
        </Text>
        <Text id={`${id}-last-turn`} tone="muted" className={styles.detail}>
          {lastTurnText(connector.lastTurnAt, now)}
        </Text>
      </CardContent>
    </Card>
  );
}

export function StatusPage({ status: answer }: { readonly status: AgentStatus | null }): ReactNode {
  // The page waits for the first answer: nothing for a quarter of a second, then two skeleton
  // lines for at least half a second, then the status. Any answer but `running`, and a `running` one
  // with a part that failed, is a failure and shows at once.
  const skeletonShown = useDelayedLoader(answer === null);
  const failedPart =
    answer?.state === 'running' &&
    (answer.database?.state === 'failed' || answer.construct?.state === 'failed');
  const status = skeletonShown && answer?.state === 'running' && !failedPart ? null : answer;
  return (
    <>
      <Heading typography="display" render={<h1 />}>
        {t('StatusPage.Title')}
      </Heading>
      <Card className={styles.card}>
        <CardHeader>
          <CardTitle render={<h2 />}>{t('StatusPage.Agent')}</CardTitle>
          {status !== null && (
            <CardAction className={styles.state}>
              <StatusDot tone={tones[status.state]} />
              <Badge id="agent-status" variant={variants[status.state]} data-state={status.state}>
                {status.state}
              </Badge>
            </CardAction>
          )}
        </CardHeader>
        <CardContent>
          {status === null ? (
            // The card keeps the space of the skeleton while the skeleton waits to show.
            <div className={styles.skeletonSpace}>
              {skeletonShown && (
                <div id="status-checking" className={styles.skeleton}>
                  <VisuallyHidden>{t('StatusPage.Checking')}</VisuallyHidden>
                  <Skeleton className={styles.line} />
                  <Skeleton className={styles.line} />
                </div>
              )}
            </div>
          ) : (
            <>
              <Text id="agent-detail" tone="muted" className={styles.detail}>
                {status.state === 'running'
                  ? t('StatusPage.Version', { version: status.version, pid: status.pid })
                  : status.reason}
              </Text>
              {status.state === 'running' && status.model !== undefined && (
                <Text id="agent-model" className={styles.detail}>
                  {t('StatusPage.Model', { model: status.model })}
                </Text>
              )}
            </>
          )}
        </CardContent>
      </Card>
      {status?.state === 'running' && status.database !== undefined && (
        <PartCard id="database" title={t('StatusPage.Database')} part={status.database} />
      )}
      {status?.state === 'running' && status.construct !== undefined && (
        <PartCard id="construct" title={t('StatusPage.Construct')} part={status.construct} />
      )}
      {status?.state === 'running' &&
        ((status.connectors !== undefined && status.connectors.length > 0) ||
          status.screen !== undefined) && (
          <>
            <Heading typography="heading-1" className={styles.section} render={<h2 />}>
              {t('StatusPage.Connectors')}
            </Heading>
            {(status.connectors ?? []).map((connector) => (
              <ConnectorCard key={connector.name} connector={connector} now={Date.now()} />
            ))}
            {status.screen !== undefined && <ScreenCard screen={status.screen} now={Date.now()} />}
          </>
        )}
      {status !== null && (
        <Card className={styles.card}>
          <CardHeader>
            <CardTitle render={<h2 />}>{t('StatusPage.StartAtLogin')}</CardTitle>
            <CardAction>
              <Badge
                id="start-at-login"
                variant={loginVariants[status.startAtLogin]}
                data-state={status.startAtLogin}
              >
                {t(`StatusPage.StartAtLogin.${status.startAtLogin}`)}
              </Badge>
            </CardAction>
          </CardHeader>
        </Card>
      )}
    </>
  );
}
