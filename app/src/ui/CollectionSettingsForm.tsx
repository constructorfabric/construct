import {
  Alert,
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  Heading,
  Switch,
} from '@gears-frontx/ui-kit';
import { useState } from 'react';
import type { ReactNode } from 'react';
import type { AgentStatus } from '../agent-status';
import type { CollectionInput, CollectionSettings } from '../agent-settings';
import { t } from '../translation';
import styles from './app.module.css';
import { OutcomeMessage } from './ModelSettingsForm';
import { ScreenGrants } from './ScreenGrants';

export function CollectionSettingsForm({
  settings,
  supportsScreen,
  status,
  notice,
  onSettings,
}: {
  readonly settings: CollectionSettings;
  /** Whether this computer can collect the time in apps and websites; when not, nothing of it shows. */
  readonly supportsScreen: boolean;
  readonly status: AgentStatus | null;
  readonly notice: string | null;
  readonly onSettings: (reply: {
    readonly collection: CollectionSettings;
    readonly notice: string | null;
  }) => void;
}): ReactNode {
  const [error, setError] = useState<string | null>(null);

  // A switch is saved the moment it is toggled, and shows what the agent answers, not what was asked.
  const save = async (input: CollectionInput): Promise<void> => {
    const result = await window.constructAgent.saveCollection(input);
    if (result.ok) {
      setError(null);
      onSettings(result);
    } else {
      setError(result.error);
    }
  };

  return (
    <section aria-labelledby="collection-section-title" className={styles.collection}>
      <Heading id="collection-section-title" typography="heading-1" render={<h2 />}>
        {t('CollectionSettingsForm.Title')}
      </Heading>
      {notice === null ? null : (
        <Alert id="collection-notice" className={styles.outcome} role="status">
          {notice}
        </Alert>
      )}
      <FieldGroup className={styles.form}>
        <Field orientation="horizontal">
          <FieldContent>
            <FieldLabel id="collection-enabled-label" htmlFor="collection-enabled">
              {t('CollectionSettingsForm.All')}
            </FieldLabel>
            <FieldDescription id="collection-enabled-hint">
              {t('CollectionSettingsForm.AllHint')}
            </FieldDescription>
          </FieldContent>
          <Switch
            id="collection-enabled"
            checked={settings.enabled}
            aria-labelledby="collection-enabled-label"
            aria-describedby="collection-enabled-hint"
            onCheckedChange={(enabled) => void save({ enabled })}
          />
        </Field>
        <div className={styles.tools}>
          {/* Under a master switch that is off a tool shows off and cannot be changed; its own saved value stays. */}
          <Field orientation="horizontal">
            <FieldContent>
              <FieldLabel id="collection-claude-code-label" htmlFor="collection-claude-code">
                {t('CollectionSettingsForm.ClaudeCode')}
              </FieldLabel>
            </FieldContent>
            <Switch
              id="collection-claude-code"
              checked={settings.enabled && settings.claudeCode}
              disabled={!settings.enabled}
              aria-labelledby="collection-claude-code-label"
              onCheckedChange={(claudeCode) => void save({ claudeCode })}
            />
          </Field>
          <Field orientation="horizontal">
            <FieldContent>
              <FieldLabel id="collection-codex-label" htmlFor="collection-codex">
                {t('CollectionSettingsForm.Codex')}
              </FieldLabel>
            </FieldContent>
            <Switch
              id="collection-codex"
              checked={settings.enabled && settings.codex}
              disabled={!settings.enabled}
              aria-labelledby="collection-codex-label"
              onCheckedChange={(codex) => void save({ codex })}
            />
          </Field>
        </div>
        {supportsScreen && (
          <>
            {/* Under a master switch that is off this one shows off too; its own saved value stays. */}
            <Field orientation="horizontal">
              <FieldContent>
                <FieldLabel id="collection-screen-label" htmlFor="collection-screen">
                  {t('CollectionSettingsForm.Screen')}
                </FieldLabel>
                <FieldDescription id="collection-screen-hint">
                  {t('CollectionSettingsForm.ScreenHint')}
                </FieldDescription>
              </FieldContent>
              <Switch
                id="collection-screen"
                checked={settings.enabled && settings.screen}
                disabled={!settings.enabled}
                aria-labelledby="collection-screen-label"
                aria-describedby="collection-screen-hint"
                onCheckedChange={(screen) => void save({ screen })}
              />
            </Field>
            {settings.enabled && settings.screen && <ScreenGrants status={status} />}
          </>
        )}
      </FieldGroup>
      {error === null ? null : (
        <OutcomeMessage id="collection-error" outcome={{ kind: 'error', text: error }} />
      )}
    </section>
  );
}
