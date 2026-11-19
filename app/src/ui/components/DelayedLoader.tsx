import { Spinner } from '@gears-frontx/ui-kit';
import type { ReactNode } from 'react';
import { useDelayedLoader } from './useDelayedLoader';

/**
 * What a page shows while it waits for its first answer: nothing for a quarter of a second, then
 * a spinner with `label` for at least half a second, then the content. A failure is not content:
 * the page shows it before it asks this part, so it is never held back.
 */
export function DelayedLoader({
  waiting,
  id,
  label,
  children,
}: {
  readonly waiting: boolean;
  readonly id: string;
  readonly label: string;
  readonly children: ReactNode;
}): ReactNode {
  const shown = useDelayedLoader(waiting);
  if (shown) {
    return <Spinner id={id} label={label} />;
  }
  return waiting ? null : children;
}
