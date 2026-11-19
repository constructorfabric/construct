import { Field, FieldContent, FieldDescription, FieldLabel } from '@gears-frontx/ui-kit';
import type { ReactNode } from 'react';

/**
 * A label with an optional description at the start of a row and one control at the end. The
 * label has the id `<id>-label` and the description `<id>-hint`, so the control can name itself
 * by them; a control that is not labelled by `htmlFor` (a button) is named with `aria-labelledby`.
 */
export function ControlRow({
  id,
  label,
  description,
  labelFor,
  children,
}: {
  readonly id: string;
  readonly label: string;
  readonly description?: string;
  /** The id of the control the label belongs to, when it is a form control. */
  readonly labelFor?: string;
  readonly children: ReactNode;
}): ReactNode {
  return (
    <Field orientation="horizontal">
      <FieldContent>
        <FieldLabel id={`${id}-label`} htmlFor={labelFor}>
          {label}
        </FieldLabel>
        {description === undefined ? null : (
          <FieldDescription id={`${id}-hint`}>{description}</FieldDescription>
        )}
      </FieldContent>
      {children}
    </Field>
  );
}
