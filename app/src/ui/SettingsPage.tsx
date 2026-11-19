import { Alert, Heading } from '@gears-frontx/ui-kit';
import { useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import { withCollectionReply } from '../agent-settings';
import type { SettingsResult } from '../agent-settings';
import { t } from '../translation';
import { CollectionSettingsForm } from './CollectionSettingsForm';
import { DelayedLoader } from './components/DelayedLoader';
import { DashboardSetupSection } from './DashboardSetupSection';
import { ModelSettingsForm } from './ModelSettingsForm';
import { YourDataSection } from './YourDataSection';
import type { Chat } from './useChat';
import { useDashboardSetup } from './useDashboardSetup';
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
  const setup = useDashboardSetup();
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

  const sections = loaded?.ok === true && (
    <>
      <ModelSettingsForm
        settings={loaded.model}
        onSettings={(model) =>
          setLoaded((current) => (current?.ok ? { ...current, model } : current))
        }
      />
      <DashboardSetupSection
        setup={setup}
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
  );
  // A failed read of the setup state is a failure like a failed read of the settings: the page
  // shows it at once, with its sections, and no loader is held for it.
  const setupFailed = setup.view.kind === 'unavailable';

  return (
    <>
      <Heading typography="display" render={<h1 />}>
        {t('SettingsPage.Title')}
      </Heading>
      {loaded !== null && !loaded.ok ? (
        <Alert variant="destructive">
          {t('SettingsPage.Unavailable', { reason: loaded.error })}
        </Alert>
      ) : setupFailed && loaded?.ok === true ? (
        sections
      ) : (
        <DelayedLoader
          waiting={loaded === null || setup.view.kind === 'loading'}
          id="settings-loading"
          label={t('SettingsPage.Loading')}
        >
          {sections}
        </DelayedLoader>
      )}
    </>
  );
}
