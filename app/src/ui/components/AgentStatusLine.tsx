import { StatusDot } from '@gears-frontx/ui-kit';
import type { ReactNode } from 'react';
import type { CollectionSummary } from '../../agent-status';
import { t } from '../../translation';
import { tones } from '../state-tones';
import styles from './components.module.css';

export function AgentStatusLine({
  state,
  collection,
}: {
  readonly state: keyof typeof tones;
  /** What collection is doing, when the agent is running and there is something to say. */
  readonly collection?: CollectionSummary;
}): ReactNode {
  return (
    <StatusDot
      id="sidebar-agent-state"
      className={styles.statusLine}
      data-state={state}
      tone={tones[state]}
      label={
        <>
          <span>{t('App.AgentState')}</span> <span id="sidebar-agent-state-word">{state}</span>
          {collection === undefined ? null : (
            <>
              {', '}
              <span id="sidebar-collection" data-collection={collection}>
                {t(`App.Collection.${collection}`)}
              </span>
            </>
          )}
        </>
      }
    />
  );
}
