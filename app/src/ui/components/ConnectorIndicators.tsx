import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@gears-frontx/ui-kit';
import { ChevronDown, ChevronRight } from 'lucide-react';
import type { ReactNode } from 'react';
import type { Connector, ScreenCollection } from '../../agent-status';
import { t } from '../../translation';
import { connectorName } from '../connector-name';
import { connectorTones, screenTones } from '../state-tones';
import styles from './components.module.css';
import { StatusRow } from './StatusRow';

/**
 * One indicator for each tool the agent collects from, under a group that opens and closes: a dot
 * (green when it collects or scans, grey when it is off, red when it is broken) and the state in
 * words beside the name, so the colour is never the only sign. The apps and websites collection is
 * one more row when the agent has it. A closed group still says how many rows have a problem: a tool
 * that failed, or a collection that failed or lacks a permission. Whether it is open is kept by the caller, so it survives a poll that fails.
 */
export function ConnectorIndicators({
  connectors,
  screen,
  open,
  onOpenChange,
}: {
  readonly connectors: readonly Connector[];
  readonly screen?: ScreenCollection;
  readonly open: boolean;
  readonly onOpenChange: (open: boolean) => void;
}): ReactNode {
  if (connectors.length === 0 && screen === undefined) {
    return null;
  }
  const problems =
    connectors.filter((connector) => connector.state === 'failed').length +
    (screen?.state === 'failed' || screen?.state === 'needs_permission' ? 1 : 0);
  return (
    <Collapsible open={open} onOpenChange={onOpenChange} id="sidebar-collection">
      <CollapsibleTrigger className={styles.collectionTrigger}>
        {open ? <ChevronDown aria-hidden /> : <ChevronRight aria-hidden />}
        <span>
          {t('App.Collection')}
          {!open && problems > 0 ? (
            <span id="sidebar-collection-failed" className={styles.collectionFailed}>
              {' '}
              {t('App.Collection.Problem', { count: problems })}
            </span>
          ) : null}
        </span>
      </CollapsibleTrigger>
      <CollapsibleContent>
        {connectors.map((connector) => (
          <StatusRow
            nested
            key={connector.name}
            id={`sidebar-connector-${connector.name}`}
            state={connector.state}
            tone={connectorTones[connector.state]}
            name={connectorName(connector.name)}
            word={t(`StatusPage.Connector.${connector.state}`)}
          />
        ))}
        {screen === undefined ? null : (
          <StatusRow
            nested
            id="sidebar-screen"
            state={screen.state}
            tone={screenTones[screen.state]}
            name={t('StatusPage.Screen.Title')}
            word={t(`StatusPage.Screen.${screen.state}`)}
          />
        )}
      </CollapsibleContent>
    </Collapsible>
  );
}
