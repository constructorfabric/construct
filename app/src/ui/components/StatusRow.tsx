import { StatusDot } from '@gears-frontx/ui-kit';
import type { ComponentProps, ReactNode } from 'react';
import styles from './components.module.css';

/** One row of the sidebar's status area: a dot, a name and the state in words beside it. */
export function StatusRow({
  id,
  state,
  tone,
  name,
  word,
}: {
  readonly id: string;
  readonly state: string;
  readonly tone: ComponentProps<typeof StatusDot>['tone'];
  readonly name: string;
  readonly word: string;
}): ReactNode {
  return (
    <StatusDot
      id={id}
      className={styles.statusLine}
      data-state={state}
      tone={tone}
      label={
        <>
          <span>{name}</span> <span id={`${id}-word`}>{word}</span>
        </>
      }
    />
  );
}
