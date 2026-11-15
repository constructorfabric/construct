/**
 * The address to open, or `null` when it is not one that is opened. Only web addresses are:
 * anything else an answer links to (`file:`, `javascript:`, an app's own scheme) is not.
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
