import { t } from '../translation';

const kilobyte = 1024;
const megabyte = kilobyte * 1024;

/** A size as the page shows it: `812 B`, `12.5 KB`, `3.2 MB`. */
export function formatSize(bytes: number): string {
  if (bytes < kilobyte) {
    return t('Size.Bytes', { size: Math.round(bytes) });
  }
  if (bytes < megabyte) {
    return t('Size.Kilobytes', { size: (bytes / kilobyte).toFixed(1) });
  }
  return t('Size.Megabytes', { size: (bytes / megabyte).toFixed(1) });
}
