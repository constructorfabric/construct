import { Alert, Heading, Spinner } from '@gears-frontx/ui-kit';
import { useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import type { SettingsResult } from '../agent-settings';
import { t } from '../translation';
import { ModelSettingsForm } from './ModelSettingsForm';

export function SettingsPage(): ReactNode {
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
        <ModelSettingsForm
          settings={loaded.model}
          onSettings={(model) => setLoaded({ ok: true, model })}
        />
      ) : (
        <Alert variant="destructive">
          {t('SettingsPage.Unavailable', { reason: loaded.error })}
        </Alert>
      )}
    </>
  );
}
