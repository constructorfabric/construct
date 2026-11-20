import {
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@gears-frontx/ui-kit';
import type { ReactNode } from 'react';
import styles from './components.module.css';

export interface MenuItemAction {
  /** The name a screen reader gives the action: its icon says nothing. */
  readonly label: string;
  readonly icon: ReactNode;
  readonly onSelect: () => void;
}

interface MenuItemProps {
  readonly id?: string;
  readonly icon: ReactNode;
  readonly label: string;
  /** The page or the row the person is on: it is filled and marked as the current one. */
  readonly active?: boolean;
  /** What `aria-current` says of an active item: a page of the menu, or one row of a list. */
  readonly ariaCurrent?: 'page' | 'true';
  readonly disabled?: boolean;
  /** A button at the end of the item, beside the item's own button. */
  readonly action?: MenuItemAction;
  /** Text for a tooltip that a hover or the keyboard focus opens, for a label that may be cut. */
  readonly hint?: string;
  readonly onSelect: () => void;
}

/**
 * One item of the sidebar: an icon and a label, on the grid every sidebar row shares. A label
 * longer than the item ends with an ellipsis; `hint` shows the whole of it. An item that is
 * switched off is marked `aria-disabled` and stays focusable, so its tooltip still opens, and a
 * click on it does nothing.
 */
export function MenuItem({
  id,
  icon,
  label,
  active = false,
  ariaCurrent = 'page',
  disabled = false,
  action,
  hint,
  onSelect,
}: MenuItemProps): ReactNode {
  const button = (
    <SidebarMenuButton
      isActive={active}
      render={
        <button
          id={id}
          type="button"
          className={styles.row}
          aria-current={active ? ariaCurrent : undefined}
          aria-disabled={disabled ? true : undefined}
          onClick={disabled ? undefined : onSelect}
        />
      }
    >
      {icon}
      <span>{label}</span>
    </SidebarMenuButton>
  );
  return (
    <SidebarMenuItem>
      {hint === undefined ? (
        button
      ) : (
        <Tooltip>
          <TooltipTrigger render={button} />
          <TooltipContent side="right">{hint}</TooltipContent>
        </Tooltip>
      )}
      {action === undefined ? null : (
        <SidebarMenuAction
          className={styles.action}
          aria-label={action.label}
          disabled={disabled}
          onClick={action.onSelect}
        >
          {action.icon}
        </SidebarMenuAction>
      )}
    </SidebarMenuItem>
  );
}
