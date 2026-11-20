import { isGrantName } from './screen-grants';
import type { GrantName } from './screen-grants';

/** What the window asks the main process to do for it: open a link or a settings pane, or copy some text. */
export interface ExternalApi {
  /** Opens an `http:` or `https:` address in the system browser; `false` when it refused. */
  readonly openLink: (url: string) => Promise<boolean>;
  /** Opens a pane of System Settings, named from the fixed list; `false` when it refused. */
  readonly openPane: (pane: string) => Promise<boolean>;
  /** Puts plain text on the system clipboard; `false` when it refused. */
  readonly copyText: (text: string) => Promise<boolean>;
}

export const OPEN_LINK_CHANNEL = 'external:open-link';
export const OPEN_PANE_CHANNEL = 'external:open-pane';
export const COPY_TEXT_CHANNEL = 'external:copy-text';

/** The most characters the main process puts on the clipboard for the window. */
export const maxCopyChars = 64 * 1024;

/** The text to copy, or `null` when it is not a non-empty text of a size the main process copies. */
export function copyableText(raw: unknown): string | null {
  return typeof raw === 'string' && raw !== '' && raw.length <= maxCopyChars ? raw : null;
}

/**
 * The address to open, or `null` when it is not one the main process opens. This is the gate in
 * front of the system browser: only web addresses pass, whatever the window asked for.
 */
export function webAddress(raw: unknown): string | null {
  if (typeof raw !== 'string') {
    return null;
  }
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return null;
  }
  return url.protocol === 'https:' || url.protocol === 'http:' ? url.href : null;
}

/** The panes of System Settings the main process opens, by name; the window never sends an address. */
const systemPanes: Readonly<Record<GrantName, string>> = {
  accessibility: 'Privacy_Accessibility',
  screen_recording: 'Privacy_ScreenCapture',
  automation: 'Privacy_Automation',
  input_monitoring: 'Privacy_ListenEvent',
};

/**
 * The address of the pane named `raw`, or `null` when the name is not on the fixed list. This is
 * the gate in front of the system's own address: nothing from the window is part of it.
 */
export function systemPaneAddress(raw: unknown): string | null {
  if (!isGrantName(raw)) {
    return null;
  }
  return `x-apple.systempreferences:com.apple.preference.security?${systemPanes[raw]}`;
}
