import { Spinner } from '@gears-frontx/ui-kit';
import { CircleCheck, CircleX } from 'lucide-react';
import type { ReactNode } from 'react';
import { t } from '../translation';
import styles from './app.module.css';
import { checkLabel, describeCheck } from './check-reasons';
import type { CheckRow } from './check-reasons';

/** The checks of a model Test, one row each. */
export function CheckList({ rows }: { readonly rows: readonly CheckRow[] }): ReactNode {
  return (
    <ul id="model-checks" className={styles.checks} aria-label={t('CheckList.Label')}>
      {rows.map((row) => (
        <CheckItem key={row.name} row={row} />
      ))}
    </ul>
  );
}

type CheckTone = 'running' | 'works' | 'skipped' | 'failed';

/** One row: an icon and a text, so the colour is never the only sign. */
function CheckItem({ row }: { readonly row: CheckRow }): ReactNode {
  const { check } = row;
  const tone: CheckTone =
    check === null
      ? 'running'
      : check.result === 'works' || check.result === 'skipped'
        ? check.result
        : 'failed';
  return (
    <li className={styles.check} data-check={row.name} data-state={tone}>
      <span className={styles.checkIcon} aria-hidden="true">
        {/* The row's text says what the loader means; the kit's own status role would say it twice. */}
        {tone === 'running' && <Spinner compact role="presentation" aria-label={undefined} />}
        {tone === 'works' && <CircleCheck className={styles.checkWorks} />}
        {tone === 'failed' && <CircleX className={styles.checkFailed} />}
      </span>
      <span className={styles.checkText}>
        <span className={styles.checkName}>{checkLabel(row.name)}</span>
        {t('CheckList.Separator')}
        <span className={tone === 'skipped' ? styles.checkSkipped : undefined}>
          {describeCheck(check)}
        </span>
      </span>
    </li>
  );
}
