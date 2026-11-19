import { StatusDot } from '@gears-frontx/ui-kit';
import type { ReactNode } from 'react';
import { t } from '../../translation';
import { tones } from '../state-tones';
import styles from './components.module.css';

export function AgentStatusLine({ state }: { readonly state: keyof typeof tones }): ReactNode {
  return (
    <StatusDot
      id="sidebar-agent-state"
      className={styles.statusLine}
      data-state={state}
      tone={tones[state]}
      label={
        <>
          <span>{t('App.AgentState')}</span> <span id="sidebar-agent-state-word">{state}</span>
        </>
      }
    />
  );
}
