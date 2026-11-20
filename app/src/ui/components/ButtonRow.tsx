import type { ReactNode } from 'react';
import styles from './components.module.css';

/** The buttons of a section or a form, side by side, wrapping when the window is narrow. */
export function ButtonRow({ children }: { readonly children: ReactNode }): ReactNode {
  return <div className={styles.buttonRow}>{children}</div>;
}
