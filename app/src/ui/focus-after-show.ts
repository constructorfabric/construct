let pending = false;

/** The user pressed Show on the page: the first row's button takes the focus when the rows return. */
export function expectRowsBack(): void {
  pending = true;
}

/** Whether the rows that are drawn now are the ones the user asked to see again; asks once. */
export function takeRowsBack(): boolean {
  const was = pending;
  pending = false;
  return was;
}

/** The change that was to bring the rows back was refused: no row is coming. */
export function cancelRowsBack(): void {
  pending = false;
}
