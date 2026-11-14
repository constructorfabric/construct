import { Heading, Text } from '@gears-frontx/ui-kit';
import type { ReactNode } from 'react';
import type { AgentStatus } from '../agent-status';
import { t } from '../translation';
import styles from './app.module.css';

export function StatusPage({ status }: { readonly status: AgentStatus | null }): ReactNode {
  const detail =
    status === null
      ? ''
      : status.state === 'running'
        ? t('StatusPage.Version', { version: status.version, pid: status.pid })
        : status.reason;

  return (
    <>
      <Heading typography="display" render={<h1 />}>
        {t('StatusPage.Title')}
      </Heading>
      <p id="agent-status" className={styles.state} data-state={status?.state ?? 'unknown'}>
        {status === null ? t('StatusPage.Checking') : status.state}
      </p>
      <Text id="agent-detail" tone="muted" className={styles.detail}>
        {detail}
      </Text>
    </>
  );
}
