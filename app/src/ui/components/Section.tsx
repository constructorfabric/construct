import { Alert, FieldGroup, Heading, Text } from '@gears-frontx/ui-kit';
import type { FormEvent, ReactNode } from 'react';
import { ButtonRow } from './ButtonRow';
import styles from './components.module.css';

/**
 * A titled part of a page: a heading, a line of text under it, a notice, a group of fields and
 * the buttons that act on them. With `onSubmit` the fields and the buttons form one form.
 * `after` is what follows, such as the outcome of an action.
 */
export function Section({
  titleId,
  title,
  description,
  notice,
  noticeId,
  actions,
  after,
  onSubmit,
  children,
}: {
  /** The id of the heading, which names the section. */
  readonly titleId: string;
  readonly title: string;
  readonly description?: string;
  readonly notice?: string | null;
  readonly noticeId?: string;
  readonly actions?: ReactNode;
  readonly after?: ReactNode;
  readonly onSubmit?: (event: FormEvent) => void;
  readonly children?: ReactNode;
}): ReactNode {
  const body = (
    <>
      <FieldGroup className={styles.fields}>{children}</FieldGroup>
      {actions === undefined ? null : <ButtonRow>{actions}</ButtonRow>}
    </>
  );
  return (
    <section aria-labelledby={titleId} className={styles.section}>
      <Heading id={titleId} typography="heading-1" render={<h2 />}>
        {title}
      </Heading>
      {description === undefined ? null : <Text tone="muted">{description}</Text>}
      {notice === undefined || notice === null ? null : (
        <Alert id={noticeId} className={styles.notice} role="status">
          {notice}
        </Alert>
      )}
      {onSubmit === undefined ? (
        body
      ) : (
        <form onSubmit={onSubmit} noValidate>
          {body}
        </form>
      )}
      {after}
    </section>
  );
}
