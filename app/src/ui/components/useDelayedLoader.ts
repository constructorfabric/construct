import { useEffect, useRef, useState } from 'react';
import { createDelayedFlag } from './delayed-flag';
import type { DelayedFlag } from './delayed-flag';

/**
 * Whether the loader of a wait is on show: not before the wait has lasted a quarter of a second,
 * and, once on, not for less than half a second. Whatever the page shows in its place while this
 * is `false` (nothing, or the answer once it is there) is its own to decide.
 *
 * A change of `scope` ends the wait it stood for: a loader that was on show for the old scope is
 * gone at once, and a wait in the new scope starts its own quarter of a second. A page that
 * waits for one thing after another (one conversation after another) gives each its scope.
 */
export function useDelayedLoader(waiting: boolean, scope = ''): boolean {
  const [state, setState] = useState({ scope, shown: false });
  const flag = useRef<DelayedFlag | null>(null);

  useEffect(() => {
    // A scope starts with nothing on show, whatever the last one left behind.
    setState((current) =>
      current.scope === scope && !current.shown ? current : { scope, shown: false },
    );
    const created = createDelayedFlag((shown) => {
      setState({ scope, shown });
    });
    flag.current = created;
    return () => {
      created.dispose();
      flag.current = null;
    };
  }, [scope]);

  useEffect(() => {
    flag.current?.setWaiting(waiting);
  }, [waiting, scope]);

  return state.scope === scope && state.shown;
}
