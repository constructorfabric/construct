import { sourceValues } from '../dashboard-protocol';
import type { DashboardDocument, Source, Widget } from '../dashboard-protocol';
import type { Grant, GrantName } from '../screen-grants';

/** What a widget counts: its own source, else the page's. */
export function sourceOf(document: DashboardDocument, widget: Widget): Source {
  return widget.source ?? document.source;
}

/** The sources the widgets of the document count, the AI sessions first. */
export function sourcesUsed(document: DashboardDocument): Source[] {
  const used = new Set(document.widgets.map((widget) => sourceOf(document, widget)));
  return sourceValues.filter((source) => used.has(source));
}

/** The widgets of one source, as the document places them. */
export function sourceWidgets(document: DashboardDocument, source: Source): Widget[] {
  return document.widgets.filter((widget) => sourceOf(document, widget) === source);
}

/**
 * The widgets of one source as the grid draws them. A document with the widgets of both sources
 * stacks them one under the other, so the widgets of the source shown move up to the top; a
 * document with one source is drawn where it places the widgets.
 */
export function gridWidgets(document: DashboardDocument, source: Source): Widget[] {
  const own = sourceWidgets(document, source);
  if (sourcesUsed(document).length < 2) {
    return own;
  }
  const top = Math.min(...own.map((widget) => widget.position.row));
  return own.map((widget) => ({
    ...widget,
    position: { ...widget.position, row: widget.position.row - top },
  }));
}

/** The permissions the person has not given: a browser that is not running is not one. */
export function missingGrants(grants: readonly Grant[] | undefined): GrantName[] {
  return (grants ?? [])
    .filter((grant) => grant.state === 'denied' || grant.state === 'not_asked')
    .map((grant) => grant.name);
}
