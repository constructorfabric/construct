import { useState } from 'react';
import type { ReactNode } from 'react';
import type { AgentStatus } from '../agent-status';
import type { CollectionInput, CollectionSettings } from '../agent-settings';
import { t } from '../translation';
import styles from './app.module.css';
import { Section } from './components/Section';
import { SwitchRow } from './components/SwitchRow';
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
    <Section
      titleId="collection-section-title"
      title={t('CollectionSettingsForm.Title')}
      notice={notice}
      noticeId="collection-notice"
      after={
        error === null ? null : (
          <OutcomeMessage id="collection-error" outcome={{ kind: 'error', text: error }} />
        )
      }
    >
      <SwitchRow
        id="collection-enabled"
        label={t('CollectionSettingsForm.All')}
        description={t('CollectionSettingsForm.AllHint')}
        checked={settings.enabled}
        onChange={(enabled) => void save({ enabled })}
      />
      <div className={styles.tools}>
        {/* Under a master switch that is off a tool shows off and cannot be changed; its own saved value stays. */}
        <SwitchRow
          id="collection-claude-code"
          label={t('CollectionSettingsForm.ClaudeCode')}
          checked={settings.enabled && settings.claudeCode}
          disabled={!settings.enabled}
          onChange={(claudeCode) => void save({ claudeCode })}
        />
        <SwitchRow
          id="collection-codex"
          label={t('CollectionSettingsForm.Codex')}
          checked={settings.enabled && settings.codex}
          disabled={!settings.enabled}
          onChange={(codex) => void save({ codex })}
        />
      </div>
      {supportsScreen && (
        <>
          {/* Under a master switch that is off this one shows off too; its own saved value stays. */}
          <SwitchRow
            id="collection-screen"
            label={t('CollectionSettingsForm.Screen')}
            description={t('CollectionSettingsForm.ScreenHint')}
            checked={settings.enabled && settings.screen}
            disabled={!settings.enabled}
            onChange={(screen) => void save({ screen })}
          />
          {settings.enabled && settings.screen && <ScreenGrants status={status} />}
        </>
      )}
    </Section>
  );
}
