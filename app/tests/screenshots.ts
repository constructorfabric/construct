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
  // The kit eases colours between themes; a screenshot taken mid-way shows a mix of both. Only
  // animations that end are waited for: a shimmer repeats, and a scroll-driven one follows the
  // scroll position, so neither ever finishes.
  await page.evaluate(() =>
    Promise.all(
      document
        .getAnimations()
        .filter(
          (a) =>
            a.timeline instanceof DocumentTimeline &&
            Number.isFinite(a.effect?.getComputedTiming().endTime),
        )
        // A transition the page replaces while it runs ends by being cancelled: that is an end too.
        .map((a) =>
          a.finished.catch((error: unknown) => {
            if (!(error instanceof DOMException && error.name === 'AbortError')) {
              throw error;
            }
          }),
        ),
    ),
  );
  await page.screenshot({ path: path.join(screenshotDir, `${name}-${scheme}.png`) });
}

export async function screenshotBothThemes(page: Page, name: string): Promise<void> {
  const cdp = await page.context().newCDPSession(page);
  for (const scheme of ['light', 'dark'] as const) {
    await screenshotInTheme(page, cdp, scheme, name);
  }
}
