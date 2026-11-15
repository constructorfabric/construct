import type { CDPSession, Page } from '@playwright/test';
import path from 'node:path';

type Scheme = 'light' | 'dark';

const screenshotDir = path.resolve(__dirname, '..', 'test-results', 'screenshots');

// Neither `nativeTheme` nor `page.emulateMedia` changes the theme in an Electron window under a
// virtual display; the DevTools protocol does.
export async function screenshotInTheme(
  page: Page,
  cdp: CDPSession,
  scheme: Scheme,
  name: string,
): Promise<void> {
  await cdp.send('Emulation.setEmulatedMedia', {
    features: [{ name: 'prefers-color-scheme', value: scheme }],
  });
  // The kit eases colours between themes; a screenshot taken mid-way shows a mix of both.
  await page.evaluate(() => Promise.all(document.getAnimations().map((a) => a.finished)));
  await page.screenshot({ path: path.join(screenshotDir, `${name}-${scheme}.png`) });
}

export async function screenshotBothThemes(page: Page, name: string): Promise<void> {
  const cdp = await page.context().newCDPSession(page);
  for (const scheme of ['light', 'dark'] as const) {
    await screenshotInTheme(page, cdp, scheme, name);
  }
}
