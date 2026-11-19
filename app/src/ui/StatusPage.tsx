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
import type { ReactNode } from 'react';
import type { AgentStatus, Part } from '../agent-status';
import { t } from '../translation';
import styles from './app.module.css';
import { useDelayedLoader } from './components/useDelayedLoader';
import { tones } from './state-tones';

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
