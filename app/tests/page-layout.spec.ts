import type { Locator, Page } from '@playwright/test';
import { configure, input, test } from './chat-helpers';
import { expect } from './fixtures';
import type { OpenedApp } from './fixtures';
import { screenshotBothThemes } from './screenshots';

const sizes = [
  { name: 'smallest', width: 720, height: 480 },
  { name: 'initial', width: 960, height: 640 },
  { name: 'full-screen', width: 1920, height: 1080 },
] as const;

/**
 * Gives the window this content size and waits until the page reports the width. The height is
 * not checked: a screen shorter than the window keeps it shorter.
 */
async function resize(
  app: OpenedApp['app'],
  page: Page,
  width: number,
  height: number,
): Promise<void> {
  await app.evaluate(
    ({ BrowserWindow }, size) => {
      BrowserWindow.getAllWindows()[0]?.setContentSize(size.width, size.height);
    },
    { width, height },
  );
  await expect.poll(() => page.evaluate(() => window.innerWidth)).toBe(width);
}

/** How far a box starts from the left and top of the page's main area, in pixels. */
async function offset(page: Page, target: Locator): Promise<{ left: number; top: number }> {
  const box = await target.boundingBox();
  const main = await page.locator('main').boundingBox();
  if (box === null || main === null) {
    throw new Error('the box is not on the page');
  }
  return { left: box.x - main.x, top: box.y - main.y };
}

/** The switch with this name. */
const toggle = (page: Page, name: string) => page.getByRole('switch', { name });

const openPage = (page: Page, name: string) =>
  page.getByRole('button', { name, exact: true }).click();

test('every page keeps its content the same distance from the edge of the window', async ({
  app,
  page,
  provider,
}) => {
  await configure(page, { address: provider.url });
  await resize(app, page, 720, 480);

  const starts: Record<string, { left: number; top?: number }> = {};
  for (const name of ['Dashboard', 'Memory', 'Status', 'Settings']) {
    await openPage(page, name);
    const heading = page.locator('main h1');
    await expect(heading).toBeVisible();
    starts[name] = await offset(page, heading);
  }
  await openPage(page, 'Chat');
  await expect(input(page)).toBeVisible();
  // The chat keeps its own scrolling area, so only its sides are compared.
  starts.Chat = { left: (await offset(page, input(page).locator('xpath=..'))).left };

  const left = starts.Settings?.left ?? Number.NaN;
  const top = starts.Settings?.top ?? Number.NaN;
  expect(left).toBeGreaterThan(0);
  for (const name of Object.keys(starts)) {
    expect(starts[name]?.left, `${name} left`).toBeCloseTo(left, 1);
    if (name !== 'Chat') {
      expect(starts[name]?.top, `${name} top`).toBeCloseTo(top, 1);
    }
  }
});

test('the fields, buttons and switches of Settings share their edges and widths', async ({
  app,
  page,
  provider,
}) => {
  await configure(page, { address: provider.url });
  await openPage(page, 'Settings');
  await expect(toggle(page, 'Collect from my coding tools')).toBeVisible();

  for (const size of sizes) {
    await resize(app, page, size.width, size.height);
    await expect(page.locator('#model-base-url')).toBeVisible();
    const box = async (locator: Locator) => {
      const found = await locator.boundingBox();
      if (found === null) {
        throw new Error('the box is not on the page');
      }
      return found;
    };
    const fields = [
      await box(page.locator('#model-base-url')),
      await box(page.locator('#model-name')),
      await box(page.locator('#model-api-key')),
    ];
    for (const field of fields) {
      expect(field.x, `${size.name} field left`).toBeCloseTo(fields[0]?.x ?? 0, 1);
      expect(field.width, `${size.name} field width`).toBeCloseTo(fields[0]?.width ?? 0, 1);
    }
    const save = await box(page.locator('#model-save'));
    expect(save.x, `${size.name} button left`).toBeCloseTo(fields[0]?.x ?? 0, 1);

    const switches = [
      await box(toggle(page, 'Collect from my coding tools')),
      await box(toggle(page, 'Claude Code')),
      await box(toggle(page, 'Codex')),
    ];
    const fieldRight = (fields[0]?.x ?? 0) + (fields[0]?.width ?? 0);
    for (const one of switches) {
      expect(one.x + one.width, `${size.name} switch right`).toBeCloseTo(fieldRight, 1);
      expect(one.width, `${size.name} switch width`).toBeCloseTo(switches[0]?.width ?? 0, 1);
    }
    // The buttons of "Your data" end where the switches do.
    const erase = await box(page.locator('#erase-everything'));
    expect(erase.x + erase.width, `${size.name} button right`).toBeCloseTo(fieldRight, 1);

    await screenshotBothThemes(page, `settings-${size.name}`);
  }
});
