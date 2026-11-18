import type { TestCheck, TestProgress } from '../agent-settings';
import { t } from '../translation';

/** A check of the running Test: `check` stays `null` until the agent sends its line. */
export type CheckRow = { readonly name: string; readonly check: TestCheck | null };

/** The rows after `progress`: the names open the list, a check line fills its row. */
export function withProgress(
  rows: readonly CheckRow[],
  progress: TestProgress,
): readonly CheckRow[] {
  if (progress.event === 'checks') {
    return progress.names.map((name) => ({ name, check: null }));
  }
  const { check } = progress;
  return rows.map((row) => (row.name === check.name ? { ...row, check } : row));
}

/** The name of a check as the window shows it; a name the window does not know is shown as sent. */
export function checkLabel(name: string): string {
  switch (name) {
    case 'answers':
      return t('CheckList.NameAnswers');
    case 'uses tools':
      return t('CheckList.NameUsesTools');
    default:
      return name;
  }
}

/** What a check came to, in a short plain text. */
export function describeCheck(check: TestCheck | null): string {
  if (check === null) {
    return t('CheckList.Running');
  }
  switch (check.result) {
    case 'works':
      return t('CheckList.Works');
    case 'skipped':
      return t('CheckList.Skipped');
    case 'key refused':
      return t('CheckList.KeyRefused');
    case 'model not found':
      return t('CheckList.ModelNotFound');
    case 'unreachable':
      return t('CheckList.Unreachable');
    case 'timeout':
      return t('CheckList.Timeout');
    case 'error':
      return t('CheckList.Status', { status: check.status });
    case 'tools refused':
      return t('CheckList.ToolsRefused');
    case 'no tool call':
      return t('CheckList.NoToolCall');
    case 'bad tool call':
      return t('CheckList.BadToolCall');
    case 'wrong answer':
      return t('CheckList.WrongAnswer');
  }
}
