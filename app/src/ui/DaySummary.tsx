import { Button } from '@gears-frontx/ui-kit';
import { useState } from 'react';
import type { ReactNode } from 'react';
import { t } from '../translation';
import styles from './app.module.css';

/** One line for each ticket of the day, and a button that copies the lines as plain text. */
export function DaySummary({
  id,
  lines,
}: {
  readonly id: string;
  readonly lines: readonly string[];
}): ReactNode {
  // What the last press of Copy did; it goes away when the lines change.
  const [copied, setCopied] = useState<{ lines: string; done: boolean } | null>(null);
  const text = lines.join('\n');
  const state = copied?.lines === text ? copied.done : null;
  if (lines.length === 0) {
    return <p className={styles.tableEmpty}>{t('DashboardPage.Summary.Empty')}</p>;
  }
  return (
    <div id={id} className={styles.summary}>
      <ul className={styles.summaryLines}>
        {lines.map((line, index) => (
          <li key={`${index}-${line}`}>{line}</li>
        ))}
      </ul>
      <div className={styles.summaryActions}>
        <Button
          id={`${id}-copy`}
          variant="outline"
          size="sm"
          onClick={() => {
            window.constructAgent.copyText(text).then(
              (done) => {
                setCopied({ lines: text, done });
              },
              () => {
                setCopied({ lines: text, done: false });
              },
            );
          }}
        >
          {t('DashboardPage.Summary.Copy')}
        </Button>
        <span role="status" className={styles.tableEmpty}>
          {state === null
            ? ''
            : state
              ? t('DashboardPage.Summary.Copied')
              : t('DashboardPage.Summary.CopyFailed')}
        </span>
      </div>
    </div>
  );
}
