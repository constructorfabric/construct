import type { ReactNode } from 'react';
import { MenuItem } from './MenuItem';
import type { MenuItemAction } from './MenuItem';

interface ListRowProps {
  readonly icon: ReactNode;
  readonly title: string;
  /** The row the person has open. */
  readonly active?: boolean;
  readonly disabled?: boolean;
  readonly action: MenuItemAction;
  readonly onSelect: () => void;
}

/**
 * One row of a list in the sidebar: an icon, a title and an action at the end. It is a menu
 * item that marks the open row as the current one and shows a long title whole in a tooltip.
 */
export function ListRow({ title, ...row }: ListRowProps): ReactNode {
  return <MenuItem {...row} label={title} hint={title} ariaCurrent="true" />;
}
