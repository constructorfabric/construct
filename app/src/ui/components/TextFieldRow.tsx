import { Field, FieldDescription, FieldError, FieldLabel, Input } from '@gears-frontx/ui-kit';
import type { ReactNode } from 'react';

/** A labelled text input with an optional hint and, when `error` is set, its message. */
export function TextFieldRow({
  id,
  label,
  value,
  type,
  placeholder,
  hint,
  error,
  disabled,
  onChange,
  onBlur,
}: {
  readonly id: string;
  readonly label: string;
  readonly value: string;
  readonly type?: 'password';
  readonly placeholder?: string;
  readonly hint?: string;
  readonly error?: string;
  readonly disabled?: boolean;
  readonly onChange: (value: string) => void;
  readonly onBlur?: () => void;
}): ReactNode {
  const invalid = error !== undefined;
  return (
    <Field data-invalid={invalid || undefined}>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Input
        id={id}
        type={type}
        value={value}
        placeholder={placeholder}
        disabled={disabled}
        aria-invalid={invalid || undefined}
        onValueChange={onChange}
        onBlur={onBlur}
      />
      {hint === undefined ? null : <FieldDescription>{hint}</FieldDescription>}
      {invalid && <FieldError errors={[{ message: error }]} />}
    </Field>
  );
}
