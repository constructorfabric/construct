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
import type { AgentStatus } from '../agent-status';
import { t } from '../translation';
import styles from './app.module.css';
import { tones } from './state-tones';

const loginVariants = {
  registered: 'success',
  not_registered: 'secondary',
  unknown: 'secondary',
  not_supported: 'secondary',
} as const;

const variants = { running: 'success', stopped: 'danger', unknown: 'secondary' } as const;

export function StatusPage({ status }: { readonly status: AgentStatus | null }): ReactNode {
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
            <div className={styles.skeleton}>
              <VisuallyHidden>{t('StatusPage.Checking')}</VisuallyHidden>
              <Skeleton className={styles.line} />
              <Skeleton className={styles.line} />
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
