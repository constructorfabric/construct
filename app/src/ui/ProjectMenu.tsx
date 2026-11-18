import {
  Button,
  Dialog,
  DialogClose,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  Field,
  FieldLabel,
  Input,
} from '@gears-frontx/ui-kit';
import { Ellipsis } from 'lucide-react';
import { useState } from 'react';
import type { FormEvent, ReactNode } from 'react';
import { maxNameChars } from '../dashboard-protocol';
import { t } from '../translation';
import styles from './app.module.css';

export interface ProjectActions {
  readonly hide: (name: string) => void;
  readonly rename: (name: string, to: string) => void;
}

/** The form is mounted while the dialog is open, so each opening starts from the row's name. */
function RenameForm({
  name,
  onRename,
  onDone,
}: {
  readonly name: string;
  readonly onRename: (to: string) => void;
  readonly onDone: () => void;
}): ReactNode {
  const [value, setValue] = useState(name);
  const to = value.trim();
  // The Save button is disabled for an empty or an unchanged name, and a form whose default
  // button is disabled is not submitted by Enter, so a name that gets here is a new one.
  const submit = (event: FormEvent): void => {
    event.preventDefault();
    onRename(to);
    onDone();
  };
  return (
    <form onSubmit={submit} className={styles.renameForm}>
      <DialogHeader>
        <DialogTitle>{t('DashboardPage.Rename.Title', { name })}</DialogTitle>
      </DialogHeader>
      <Field>
        <FieldLabel htmlFor="dashboard-rename-name">{t('DashboardPage.Rename.Name')}</FieldLabel>
        <Input
          id="dashboard-rename-name"
          value={value}
          onValueChange={setValue}
          maxLength={maxNameChars}
          autoComplete="off"
        />
      </Field>
      <DialogFooter>
        <DialogClose render={<Button id="dashboard-rename-cancel" variant="outline" />}>
          {t('DashboardPage.Rename.Cancel')}
        </DialogClose>
        <Button id="dashboard-rename-save" type="submit" disabled={to === '' || to === name}>
          {t('DashboardPage.Rename.Save')}
        </Button>
      </DialogFooter>
    </form>
  );
}

export function ProjectMenu({
  name,
  actions,
}: {
  readonly name: string;
  readonly actions: ProjectActions;
}): ReactNode {
  const [renaming, setRenaming] = useState(false);
  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button
              variant="ghost"
              size="sm"
              data-project={name}
              aria-label={t('DashboardPage.Project.Actions', { name })}
              icon={<Ellipsis />}
            />
          }
        />
        <DropdownMenuContent align="end">
          <DropdownMenuItem onClick={() => actions.hide(name)}>
            {t('DashboardPage.Project.Hide')}
          </DropdownMenuItem>
          <DropdownMenuItem onClick={() => setRenaming(true)}>
            {t('DashboardPage.Project.Rename')}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <Dialog open={renaming} onOpenChange={setRenaming}>
        <DialogContent>
          <RenameForm
            name={name}
            onRename={(to) => actions.rename(name, to)}
            onDone={() => setRenaming(false)}
          />
        </DialogContent>
      </Dialog>
    </>
  );
}
