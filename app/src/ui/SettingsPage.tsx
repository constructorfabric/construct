import { Alert, Heading, Spinner } from '@gears-frontx/ui-kit';
import { useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import { withCollectionReply } from '../agent-settings';
import type { SettingsResult } from '../agent-settings';
import { t } from '../translation';
import { CollectionSettingsForm } from './CollectionSettingsForm';
import { DashboardSetupSection } from './DashboardSetupSection';
import { ModelSettingsForm } from './ModelSettingsForm';

export function SettingsPage({
  onOpenDashboard,
}: {
  readonly onOpenDashboard: () => void;
}): ReactNode {
  const [loaded, setLoaded] = useState<SettingsResult | null>(null);

  useEffect(() => {
    let cancelled = false;
    void window.constructAgent.getSettings().then((result) => {
      if (!cancelled) {
        setLoaded(result);
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <>
      <Heading typography="display" render={<h1 />}>
        {t('SettingsPage.Title')}
      </Heading>
      {loaded === null ? (
        <Spinner label={t('SettingsPage.Loading')} />
      ) : loaded.ok ? (
        <>
          <ModelSettingsForm
            settings={loaded.model}
            onSettings={(model) =>
              setLoaded((current) => (current?.ok ? { ...current, model } : current))
            }
          />
          <DashboardSetupSection
            modelSet={
              loaded.model.apiKeySet && loaded.model.baseUrl !== '' && loaded.model.model !== ''
            }
            collecting={loaded.collection.enabled}
            onOpenDashboard={onOpenDashboard}
          />
          <CollectionSettingsForm
            settings={loaded.collection}
            notice={loaded.notice}
            onSettings={(reply) => setLoaded((current) => withCollectionReply(current, reply))}
          />
        </>
      ) : (
        <Alert variant="destructive">
          {t('SettingsPage.Unavailable', { reason: loaded.error })}
        </Alert>
      )}
    </>
  );
}
