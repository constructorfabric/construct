import type { ReactNode } from 'react';
import { t } from '../../translation';
import { tones } from '../state-tones';
import { StatusRow } from './StatusRow';

export function AgentStatusLine({ state }: { readonly state: keyof typeof tones }): ReactNode {
  return (
    <StatusRow
      id="sidebar-agent-state"
      state={state}
      tone={tones[state]}
      name={t('App.AgentState')}
      word={state}
    />
  );
}
