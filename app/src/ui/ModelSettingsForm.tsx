import {
  Alert,
  Button,
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
  Heading,
  Input,
} from '@gears-frontx/ui-kit';
import { useState } from 'react';
import type { FormEvent, ReactNode } from 'react';
import { DEFAULT_BASE_URL, DEFAULT_MODEL } from '../agent-settings';
import type { ModelInput, ModelSettings, ModelTestResult } from '../agent-settings';
import { t } from '../translation';
import styles from './app.module.css';
import { CheckList } from './CheckList';
import { withProgress } from './check-reasons';
import type { CheckRow } from './check-reasons';

/** Shown in the empty key field of a saved key. The key itself never reaches the window. */
const savedKeyDots = '••••••••';

type Busy = 'saving' | 'testing' | 'removing' | null;

type Outcome =
  | { readonly kind: 'saved' }
  | { readonly kind: 'key-removed' }
  | { readonly kind: 'test'; readonly result: ModelTestResult }
  | { readonly kind: 'error'; readonly text: string };

type FormValues = {
  readonly baseUrl: string;
  readonly model: string;
  readonly apiKey: string;
};

type Required = 'baseUrl' | 'model';

function formValues(settings: ModelSettings): FormValues {
  return {
    baseUrl: settings.baseUrl === '' ? DEFAULT_BASE_URL : settings.baseUrl,
    model: settings.model === '' ? DEFAULT_MODEL : settings.model,
    apiKey: '',
  };
}

/** A key field nobody touched sends no key, so the agent keeps the saved one. */
function modelInput(values: FormValues): ModelInput {
  const base = { baseUrl: values.baseUrl.trim(), model: values.model.trim() };
  return values.apiKey === '' ? base : { ...base, apiKey: values.apiKey };
}

export function ModelSettingsForm({
  settings,
  onSettings,
}: {
  readonly settings: ModelSettings;
  readonly onSettings: (settings: ModelSettings) => void;
}): ReactNode {
  const [values, setValues] = useState<FormValues>(() => formValues(settings));
  // A field shows its error once it was edited, left, or a Save was tried.
  const [touched, setTouched] = useState<ReadonlySet<Required>>(new Set());
  const [busy, setBusy] = useState<Busy>(null);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  // The checks of the last Test; they stay on the page with its outcome.
  const [rows, setRows] = useState<readonly CheckRow[] | null>(null);

  const touch = (...names: Required[]): void => {
    setTouched((current) => new Set([...current, ...names]));
  };
  const edit = (name: keyof FormValues, value: string): void => {
    setValues((current) => ({ ...current, [name]: value }));
    if (name !== 'apiKey') {
      touch(name);
    }
  };
  const errorOf = (name: Required): { readonly message: string }[] =>
    touched.has(name) && values[name].trim() === ''
      ? [{ message: t('ModelSettingsForm.Required') }]
      : [];

  const save = async (event: FormEvent): Promise<void> => {
    event.preventDefault();
    touch('baseUrl', 'model');
    if (values.baseUrl.trim() === '' || values.model.trim() === '') {
      return;
    }
    setBusy('saving');
    setOutcome(null);
    setRows(null);
    const result = await window.constructAgent.saveModel(modelInput(values));
    setBusy(null);
    if (result.ok) {
      // The typed key leaves the form here; the window keeps only "a key is set".
      setValues(formValues(result.model));
      setTouched(new Set());
      onSettings(result.model);
      setOutcome({ kind: 'saved' });
    } else {
      setOutcome({ kind: 'error', text: result.error });
    }
  };

  const test = async (): Promise<void> => {
    setBusy('testing');
    setOutcome(null);
    setRows(null);
    const stopListening = window.constructAgent.onTestProgress((progress) => {
      setRows((current) => withProgress(current ?? [], progress));
    });
    let result: ModelTestResult;
    try {
      result = await window.constructAgent.testModel(modelInput(values));
    } finally {
      stopListening();
    }
    setBusy(null);
    // A failed Test has no checks; one that did not finish leaves no row waiting.
    setRows(
      result.checks.length > 0 ? result.checks.map((check) => ({ name: check.name, check })) : null,
    );
    setOutcome({ kind: 'test', result });
  };

  const removeKey = async (): Promise<void> => {
    setBusy('removing');
    setOutcome(null);
    setRows(null);
    const result = await window.constructAgent.removeKey();
    setBusy(null);
    if (result.ok) {
      setValues((current) => ({ ...current, apiKey: '' }));
      onSettings(result.model);
      setOutcome({ kind: 'key-removed' });
    } else {
      setOutcome({ kind: 'error', text: result.error });
    }
  };

  return (
    <section aria-labelledby="model-section-title" className={styles.section}>
      <Heading id="model-section-title" typography="heading-1" render={<h2 />}>
        {t('ModelSettingsForm.Title')}
      </Heading>
      <form className={styles.form} onSubmit={(event) => void save(event)} noValidate>
        <FieldGroup>
          <Field data-invalid={errorOf('baseUrl').length > 0 || undefined}>
            <FieldLabel htmlFor="model-base-url">{t('ModelSettingsForm.Address')}</FieldLabel>
            <Input
              id="model-base-url"
              value={values.baseUrl}
              onChange={(event) => edit('baseUrl', event.target.value)}
              onBlur={() => touch('baseUrl')}
              aria-invalid={errorOf('baseUrl').length > 0 || undefined}
              disabled={busy !== null}
            />
            <FieldError errors={errorOf('baseUrl')} />
          </Field>
          <Field data-invalid={errorOf('model').length > 0 || undefined}>
            <FieldLabel htmlFor="model-name">{t('ModelSettingsForm.Model')}</FieldLabel>
            <Input
              id="model-name"
              value={values.model}
              onChange={(event) => edit('model', event.target.value)}
              onBlur={() => touch('model')}
              aria-invalid={errorOf('model').length > 0 || undefined}
              disabled={busy !== null}
            />
            <FieldError errors={errorOf('model')} />
          </Field>
          <Field>
            <FieldLabel htmlFor="model-api-key">{t('ModelSettingsForm.ApiKey')}</FieldLabel>
            <Input
              id="model-api-key"
              type="password"
              value={values.apiKey}
              placeholder={settings.apiKeySet ? savedKeyDots : undefined}
              onChange={(event) => edit('apiKey', event.target.value)}
              disabled={busy !== null}
            />
            {settings.apiKeySet && (
              <FieldDescription>{t('ModelSettingsForm.KeySavedHint')}</FieldDescription>
            )}
          </Field>
        </FieldGroup>
        <div className={styles.actions}>
          <Button
            id="model-save"
            type="submit"
            loading={busy === 'saving'}
            disabled={busy !== null}
          >
            {t('ModelSettingsForm.Save')}
          </Button>
          <Button
            id="model-test"
            type="button"
            variant="secondary"
            loading={busy === 'testing'}
            disabled={busy !== null}
            onClick={() => void test()}
          >
            {t('ModelSettingsForm.Test')}
          </Button>
          {settings.apiKeySet && (
            <Button
              id="model-remove-key"
              type="button"
              variant="destructive"
              loading={busy === 'removing'}
              disabled={busy !== null}
              onClick={() => void removeKey()}
            >
              {t('ModelSettingsForm.RemoveKey')}
            </Button>
          )}
        </div>
      </form>
      {rows !== null && <CheckList rows={rows} />}
      {outcome !== null && <OutcomeMessage id="model-outcome" outcome={outcome} />}
    </section>
  );
}

export function OutcomeMessage({
  id,
  outcome,
}: {
  readonly id: string;
  readonly outcome: Outcome;
}): ReactNode {
  const { text, ok } = describe(outcome);
  return (
    <Alert
      id={id}
      className={styles.outcome}
      variant={ok ? 'default' : 'destructive'}
      role={ok ? 'status' : 'alert'}
    >
      {text}
    </Alert>
  );
}

type Description = { readonly text: string; readonly ok: boolean };

function describe(outcome: Outcome): Description {
  switch (outcome.kind) {
    case 'saved':
      return { text: t('OutcomeMessage.Saved'), ok: true };
    case 'key-removed':
      return { text: t('OutcomeMessage.KeyRemoved'), ok: true };
    case 'error':
      return { text: t('OutcomeMessage.Refused', { reason: outcome.text }), ok: false };
    case 'test':
      return describeTest(outcome.result);
  }
}

function describeTest(result: ModelTestResult): Description {
  switch (result.result) {
    case 'works':
      return { text: t('OutcomeMessage.Works', { answer: result.text }), ok: true };
    case 'key refused':
      return { text: t('OutcomeMessage.KeyRefused'), ok: false };
    case 'model not found':
      return { text: t('OutcomeMessage.ModelNotFound'), ok: false };
    case 'unreachable':
      return { text: t('OutcomeMessage.Unreachable'), ok: false };
    case 'timeout':
      return { text: t('OutcomeMessage.Timeout'), ok: false };
    case 'error':
      return { text: t('OutcomeMessage.Status', { status: result.status }), ok: false };
    case 'tools refused':
      return { text: t('OutcomeMessage.ToolsRefused'), ok: false };
    case 'no tool call':
      return { text: t('OutcomeMessage.NoToolCall'), ok: false };
    case 'bad tool call':
      return { text: t('OutcomeMessage.BadToolCall'), ok: false };
    case 'wrong answer':
      return { text: t('OutcomeMessage.WrongAnswer'), ok: false };
    case 'failed':
      return { text: t('OutcomeMessage.Refused', { reason: result.error }), ok: false };
  }
}
