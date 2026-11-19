import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@gears-frontx/ui-kit';
import { ChevronDown, ChevronRight } from 'lucide-react';
import type { ReactNode } from 'react';
import type { Connector } from '../../agent-status';
import { t } from '../../translation';
import { connectorName } from '../connector-name';
import { connectorTones } from '../state-tones';
import styles from './components.module.css';
import { StatusRow } from './StatusRow';

/**
 * One indicator for each tool the agent collects from, under a group that opens and closes: a dot
 * (green when it collects or scans, grey when it is off, red when it is broken) and the state in
 * words beside the name, so the colour is never the only sign. A closed group still says how many
 * tools are broken. Whether it is open is kept by the caller, so it survives a poll that fails.
 */
export function ConnectorIndicators({
  connectors,
  open,
  onOpenChange,
}: {
  readonly connectors: readonly Connector[];
  readonly open: boolean;
  readonly onOpenChange: (open: boolean) => void;
}): ReactNode {
  if (connectors.length === 0) {
    return null;
  }
  const failed = connectors.filter((connector) => connector.state === 'failed').length;
  return (
    <Collapsible open={open} onOpenChange={onOpenChange} id="sidebar-collection">
      <CollapsibleTrigger className={styles.collectionTrigger}>
        {open ? <ChevronDown aria-hidden /> : <ChevronRight aria-hidden />}
        <span>
          {t('App.Collection')}
          {!open && failed > 0 ? (
            <span id="sidebar-collection-failed" className={styles.collectionFailed}>
              {' '}
              {t('App.Collection.Failed', { count: failed })}
            </span>
          ) : null}
        </span>
      </CollapsibleTrigger>
      <CollapsibleContent>
        {connectors.map((connector) => (
          <StatusRow
            key={connector.name}
            id={`sidebar-connector-${connector.name}`}
            state={connector.state}
            tone={connectorTones[connector.state]}
            name={connectorName(connector.name)}
            word={t(`StatusPage.Connector.${connector.state}`)}
          />
        ))}
      </CollapsibleContent>
    </Collapsible>
  );
}
