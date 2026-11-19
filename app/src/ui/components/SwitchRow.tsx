import { Switch } from '@gears-frontx/ui-kit';
import type { ReactNode } from 'react';
import { ControlRow } from './ControlRow';

/** A switch with its label and an optional description. */
export function SwitchRow({
  id,
  label,
  description,
  checked,
  disabled,
  onChange,
}: {
  readonly id: string;
  readonly label: string;
  readonly description?: string;
  readonly checked: boolean;
  readonly disabled?: boolean;
  readonly onChange: (checked: boolean) => void;
}): ReactNode {
  return (
    <ControlRow id={id} label={label} description={description} labelFor={id}>
      <Switch
        id={id}
        checked={checked}
        disabled={disabled}
        aria-labelledby={`${id}-label`}
        aria-describedby={description === undefined ? undefined : `${id}-hint`}
        onCheckedChange={onChange}
      />
    </ControlRow>
  );
}
