/** What the window asks the main process to do for it: open a link. */
export interface ExternalApi {
  /** Opens an `http:` or `https:` address in the system browser; `false` when it refused. */
  readonly openLink: (url: string) => Promise<boolean>;
}

export const OPEN_LINK_CHANNEL = 'external:open-link';

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
