import { Badge, Button, StatusDot, Text } from '@gears-frontx/ui-kit';
import { useState } from 'react';
import type { ReactNode } from 'react';
import type { AgentStatus } from '../agent-status';
import type { Grant, GrantState } from '../screen-grants';
import { t } from '../translation';
import styles from './app.module.css';

const badgeVariants = {
  granted: 'success',
  denied: 'danger',
  not_asked: 'danger',
  browser_not_running: 'secondary',
} as const;

const dotTones = {
  granted: 'success',
  denied: 'danger',
  not_asked: 'danger',
  browser_not_running: 'neutral',
} as const satisfies Record<GrantState, string>;

function GrantRow({
  grant,
  onFailed,
}: {
  readonly grant: Grant;
  readonly onFailed: (failed: boolean) => void;
}): ReactNode {
  const name = t(`ScreenGrants.Name.${grant.name}`);
  return (
    <li className={styles.grant} id={`screen-grant-${grant.name}`} data-state={grant.state}>
      <div className={styles.grantText}>
        <Text className={styles.grantName}>{name}</Text>
        <Text tone="muted" className={styles.detail}>
          {t(`ScreenGrants.Reads.${grant.name}`)}
        </Text>
      </div>
      {/* The word says the state; the dot and the badge colour only back it up. */}
      <span className={styles.state}>
        <StatusDot tone={dotTones[grant.state]} />
        <Badge
          id={`screen-grant-${grant.name}-state`}
          variant={badgeVariants[grant.state]}
          data-state={grant.state}
        >
          {t(`ScreenGrants.State.${grant.state}`)}
        </Badge>
      </span>
      <Button
        id={`screen-grant-${grant.name}-open`}
        variant="outline"
        size="sm"
        aria-label={t('ScreenGrants.OpenFor', { name })}
        onClick={() => {
          window.constructAgent.openPane(grant.name).then(
            (opened) => {
              onFailed(!opened);
            },
            () => {
              onFailed(true);
            },
          );
        }}
      >
        {t('ScreenGrants.Open')}
      </Button>
    </li>
  );
}

/**
 * The permissions the apps and websites collection needs, each with its state. The state is read
 * from the status the app already polls, so it changes by itself when the person gives or takes a
 * grant in System Settings. An empty list reads as not known: the agent sends none when it could
 * not look.
 */
export function ScreenGrants({ status }: { readonly status: AgentStatus | null }): ReactNode {
  const [openFailed, setOpenFailed] = useState(false);
  const grants = status?.state === 'running' ? status.grants : undefined;

  return (
    <div className={styles.tools}>
      {grants === undefined || grants.length === 0 ? (
        status === null ? null : (
          <Text id="screen-grants-unknown" tone="muted" className={styles.detail}>
            {t('ScreenGrants.Unknown')}
          </Text>
        )
      ) : (
        <ul className={styles.grants} aria-label={t('ScreenGrants.Title')} id="screen-grants">
          {grants.map((grant) => (
            <GrantRow key={grant.name} grant={grant} onFailed={setOpenFailed} />
          ))}
        </ul>
      )}
      {openFailed && (
        <Text id="screen-grants-open-failed" role="status" tone="muted" className={styles.detail}>
          {t('ScreenGrants.OpenFailed')}
        </Text>
      )}
    </div>
  );
}
