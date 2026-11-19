import { SidebarInset } from '@gears-frontx/ui-kit';
import type { ReactNode } from 'react';
import styles from './components.module.css';

/**
 * The frame every page sits in: one padding, one width, and a scroll of its own. A `page` scrolls
 * inside the frame, in a column of a set width; a `chat` fills the window and keeps its own
 * scrolling area, which takes the frame's padding from `--page-padding`.
 */
export function PageFrame({
  layout = 'page',
  banner,
  children,
}: {
  readonly layout?: 'page' | 'chat';
  /** A notice above the page, such as a warning that concerns the whole app. */
  readonly banner?: ReactNode;
  readonly children: ReactNode;
}): ReactNode {
  const chat = layout === 'chat';
  return (
    <SidebarInset
      render={<main className={chat ? `${styles.frame} ${styles.frameChat}` : styles.frame} />}
    >
      {banner ? <div className={styles.banner}>{banner}</div> : null}
      {chat ? children : <div className={styles.column}>{children}</div>}
    </SidebarInset>
  );
}
