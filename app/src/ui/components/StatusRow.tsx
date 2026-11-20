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
  nested = false,
}: {
  readonly id: string;
  readonly state: string;
  readonly tone: ComponentProps<typeof StatusDot>['tone'];
  readonly name: string;
  readonly word: string;
  /** The row belongs to a group above it: it is indented one step, under the group's name. */
  readonly nested?: boolean;
}): ReactNode {
  return (
    <StatusDot
      id={id}
      className={nested ? `${styles.statusLine} ${styles.statusNested}` : styles.statusLine}
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
