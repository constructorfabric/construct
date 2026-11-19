import { Alert, Heading, Spinner } from '@gears-frontx/ui-kit';
import { useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import { withCollectionReply } from '../agent-settings';
import type { SettingsResult } from '../agent-settings';
import { t } from '../translation';
import { CollectionSettingsForm } from './CollectionSettingsForm';
import { DashboardSetupSection } from './DashboardSetupSection';
import { ModelSettingsForm } from './ModelSettingsForm';
import { YourDataSection } from './YourDataSection';
import type { Chat } from './useChat';
import type { Erasing } from './useErasing';

export function SettingsPage({
  chat,
  erasing,
  onOpenDashboard,
}: {
  readonly chat: Chat;
  readonly erasing: Erasing;
  readonly onOpenDashboard: () => void;
}): ReactNode {
  const [loaded, setLoaded] = useState<SettingsResult | null>(null);
  const { look } = erasing;

  // Each visit looks at the gear: an erasure may be running that the app has not seen.
  useEffect(look, [look]);

  useEffect(() => {
    let cancelled = false;
    void window.constructAgent.getSettings().then((result) => {
      if (!cancelled) {
        // A read that fails after the first one leaves the page as it was.
        setLoaded((current) => (current === null || result.ok ? result : current));
      }
    });
    return () => {
      cancelled = true;
    };
    // An erasure switches collection off: the settings are read again after each answer of the agent.
  }, [erasing.answers]);

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
          <YourDataSection chat={chat} erasure={erasing} />
        </>
      ) : (
        <Alert variant="destructive">
          {t('SettingsPage.Unavailable', { reason: loaded.error })}
        </Alert>
      )}
    </>
  );
}
