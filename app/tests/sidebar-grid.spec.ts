import type { Locator } from '@playwright/test';
import { ask, assistant, configure, rows, test } from './chat-helpers';
import { expect, runningReply, secondsAgo, withStandIn } from './fixtures';
import { screenshotBothThemes } from './screenshots';

// The agent lists a conversation under its first message cut to 60 characters.
const longMessage =
  'A question long enough that the sidebar cannot show all of it on one line without cutting it';
const longTitle = longMessage.slice(0, 60);

/** What one sidebar row looks like in the page. */
interface RowLook {
  /** The width of the row's icon in pixels; a row led by a dot has none. */
  readonly iconWidth?: number;
  readonly textSize: string;
  /** Where the row's first mark, its icon or its dot, and its text start, in pixels. */
  readonly markLeft: number;
  readonly textLeft: number;
}

async function look(row: Locator, text: Locator): Promise<RowLook> {
  const textLook = await text.evaluate((element) => {
    const box = element.getBoundingClientRect();
    return { size: getComputedStyle(element).fontSize, left: box.left };
  });
  const mark = await row.evaluate((element) => {
    const icon = element.querySelector('svg');
    const first = icon ?? element.firstElementChild;
    return {
      iconWidth: icon?.getBoundingClientRect().width,
      left: first?.getBoundingClientRect().left ?? Number.NaN,
    };
  });
  return {
    iconWidth: mark.iconWidth,
    textSize: textLook.size,
    markLeft: mark.left,
    textLeft: textLook.left,
  };
}

test('the menu, the chat list and the status line share a text size and a left edge, and the menu and the chat list an icon size', async ({
  page,
  provider,
}) => {
  provider.script = { chunks: ['an answer'], pauseMs: 0 };
  await configure(page, { address: provider.url });
  await ask(page, 'First question');
  await expect(assistant(page)).toHaveText('an answer');
  await expect(rows(page)).toHaveCount(1);
  await expect(page.locator('#sidebar-agent-state-word')).toHaveText('running');

  const menuRow = page.getByRole('button', { name: 'Chat', exact: true });
  const chatRow = rows(page).first();
  const statusRow = page.locator('#sidebar-agent-state');
  const menu = await look(menuRow, menuRow.locator('span'));
  const chat = await look(chatRow, chatRow.getByRole('button').first().locator('span'));
  const status = await look(statusRow, statusRow.getByText('Agent', { exact: true }));

  expect(menu.iconWidth).toBe(16);
  expect(chat.iconWidth).toBe(menu.iconWidth);
  expect(chat.textSize).toBe(menu.textSize);
  expect(status.textSize).toBe(menu.textSize);
  expect(chat.markLeft).toBeCloseTo(menu.markLeft, 1);
  expect(status.markLeft).toBeCloseTo(menu.markLeft, 1);
  expect(chat.textLeft).toBeCloseTo(menu.textLeft, 1);
  expect(status.textLeft).toBeCloseTo(menu.textLeft, 1);
});

test('a title too long for the sidebar is cut on screen, and the row and its tooltip hold the whole title', async ({
  page,
  provider,
}) => {
  provider.script = { chunks: ['an answer'], pauseMs: 0 };
  await configure(page, { address: provider.url });
  await ask(page, longMessage);
  await expect(assistant(page)).toHaveText('an answer');
  await expect(rows(page)).toHaveCount(1);

  const open = rows(page).first().getByRole('button').first();
  await expect(open).toHaveText(longTitle);
  const text = open.locator('span');
  expect(await text.evaluate((span) => span.scrollWidth > span.clientWidth)).toBe(true);
  expect(await text.evaluate((span) => getComputedStyle(span).textOverflow)).toBe('ellipsis');

  // The keyboard reaches the tooltip too: the title is then in the row and in the popup.
  const shown = page.getByText(longTitle, { exact: true });
  await expect(shown).toHaveCount(1);
  await expect(page.locator('#chat-new')).toBeEnabled();
  await page.mouse.move(1000, 400);
  await page.locator('#chat-new').focus();
  await page.keyboard.press('Tab');
  await expect(open).toBeFocused();
  await expect(shown).toHaveCount(2);
});

test('a row that is switched off while an answer runs takes the focus and shows its whole title', async ({
  page,
  provider,
}) => {
  provider.script = { chunks: ['an answer'], pauseMs: 0 };
  await configure(page, { address: provider.url });
  await ask(page, longMessage);
  await expect(assistant(page)).toHaveText('an answer');
  await expect(rows(page)).toHaveCount(1);

  provider.script = { chunks: ['Part ', 'never'], pauseMs: 0, holdAt: 1, forever: true };
  await ask(page, 'second');
  await expect(assistant(page).last()).toContainText('Part');
  const open = rows(page).first().getByRole('button').first();
  await expect(open).toBeDisabled();

  // A click on a switched-off item changes nothing: no new chat, and the open one stays open.
  const turns = page.locator('[data-role]');
  const before = await turns.allInnerTexts();
  await page.locator('#chat-new').click({ force: true });
  await open.click({ force: true });
  await expect(turns).toHaveText(before);
  await expect(assistant(page).last()).toContainText('Part');
  await expect(open).toHaveAttribute('aria-current', 'true');
  await expect(page.locator('#chat-new')).toBeDisabled();
  const remove = rows(page).first().getByRole('button').last();
  expect(await remove.evaluate((button) => getComputedStyle(button).opacity)).not.toBe('1');

  // The row takes the focus from the keyboard, and the whole title shows without a pointer.
  const shown = page.getByText(longTitle, { exact: true });
  await expect(shown).toHaveCount(1);
  await page.mouse.move(1000, 400);
  await page.locator('#chat-new').focus();
  await page.keyboard.press('Tab');
  await expect(open).toBeFocused();
  await expect(shown).toHaveCount(2);
  // With no fill, the ring is the only sign of the focus on a switched-off item that is not open:
  // it is drawn, and the item is not dimmed, so the ring is not either.
  await page.keyboard.press('Shift+Tab');
  await expect(page.locator('#chat-new')).toBeFocused();
  const ring = await page.locator('#chat-new').evaluate((item) => {
    const style = getComputedStyle(item);
    return {
      style: style.outlineStyle,
      width: style.outlineWidth,
      color: style.outlineColor,
      fill: style.backgroundColor,
      opacity: style.opacity,
    };
  });
  expect(ring.style).toBe('solid');
  expect(ring.width).not.toBe('0px');
  expect(ring.color).not.toBe('rgba(0, 0, 0, 0)');
  expect(ring.fill).toBe('rgba(0, 0, 0, 0)');
  expect(ring.opacity).toBe('1');
  // With the focus elsewhere, the pointer opens it too.
  await page.locator('#chat-input').focus();
  await page.mouse.move(1000, 400);
  await expect(shown).toHaveCount(1);
  await open.hover();
  await expect(shown).toHaveCount(2);
  await page.locator('#chat-stop-button').click();
  await expect(open).toBeEnabled();
});

test('the sidebar in both themes at the smallest, the initial and a full-screen window', async ({
  app,
  page,
  provider,
}) => {
  provider.script = { chunks: ['an answer'], pauseMs: 0 };
  await configure(page, { address: provider.url });
  await ask(page, 'First question');
  await expect(assistant(page)).toHaveText('an answer');
  await page.locator('#chat-new').click();
  await ask(page, longMessage);
  await expect(rows(page)).toHaveCount(2);
  await expect(page.locator('#sidebar-agent-state-word')).toHaveText('running');
  // A pointer left over New chat would read as a second selected row.
  await page.mouse.move(1000, 400);
  await page.locator('#chat-input').focus();

  for (const [name, width, height] of [
    ['smallest', 720, 480],
    ['initial', 960, 640],
    ['full-screen', 1920, 1080],
  ] as const) {
    await app.evaluate(
      ({ BrowserWindow }, size) => {
        BrowserWindow.getAllWindows()[0]?.setContentSize(size.width, size.height);
      },
      { width, height },
    );
    await page.waitForFunction((w) => window.innerWidth === w, width);
    await screenshotBothThemes(page, `sidebar-${name}`);
  }
});

test('the rows of the open Collection group stand under the group name, right of its caret, and show whole', async ({
  session,
}) => {
  const reply = runningReply({
    connectors: {
      'claude-code': { turns: 1, skipped: 0, scanning: false, last_turn: secondsAgo(5) },
      codex: { turns: 0, skipped: 0, scanning: false },
    },
    screen: { state: 'collecting', samples_today: 42, last_sample: secondsAgo(5), grants: [] },
  });
  await withStandIn(session, reply, async (page) => {
    await page.getByRole('button', { name: 'Status', exact: true }).click();
    const trigger = page.getByRole('button', { name: /^Collection/ });
    await expect(trigger).toHaveAttribute('aria-expanded', 'true');

    const menuRow = page.getByRole('button', { name: 'Chat', exact: true });
    const agentRow = page.locator('#sidebar-agent-state');
    const menu = await look(menuRow, menuRow.locator('span'));
    const agent = await look(agentRow, agentRow.getByText('Agent', { exact: true }));
    const group = await look(trigger, trigger.locator('span').first());

    for (const id of [
      'sidebar-connector-claude-code',
      'sidebar-connector-codex',
      'sidebar-screen',
    ]) {
      const row = page.locator(`#${id}`);
      const nested = await look(row, row.locator('span').first());
      // The dot is right of the group's caret, in line with the group's name.
      expect(nested.markLeft).toBeGreaterThan(group.markLeft + (group.iconWidth ?? 0));
      expect(nested.markLeft).toBeCloseTo(group.textLeft, 1);
    }
    // The Agent line is not a row of the group: it stays on the menu's grid, on the caret's edge.
    expect(agent.markLeft).toBeCloseTo(menu.markLeft, 1);
    expect(agent.markLeft).toBeCloseTo(group.markLeft, 1);

    // The longest label wraps in the narrow sidebar, and every line of it is inside its row.
    const oneLine = (await page.locator('#sidebar-connector-codex').boundingBox())?.height ?? 0;
    const screen = page.locator('#sidebar-screen');
    await expect(screen).toContainText('Apps and websites');
    await expect(page.locator('#sidebar-screen-word')).toHaveText('collecting');
    const box = await screen.boundingBox();
    expect(box?.height).toBeGreaterThan(oneLine);
    // The name and the state word are spans inside the row, and each one's own box lies in the
    // row's box, so a part pushed or cut out of the row turns this red. The row itself is a block,
    // so it also holds all of its content.
    const name = screen.getByText('Apps and websites', { exact: true });
    expect(
      await screen.evaluate((row) => ({
        height: row.scrollHeight <= row.clientHeight,
        width: row.scrollWidth <= row.clientWidth,
      })),
    ).toEqual({ height: true, width: true });
    for (const part of [name, page.locator('#sidebar-screen-word')]) {
      const within = await part.evaluate(
        (element, rowElement) => {
          const own = element.getBoundingClientRect();
          const outer = (rowElement as Element).getBoundingClientRect();
          return (
            own.width > 0 &&
            own.left >= outer.left &&
            own.right <= outer.right &&
            own.top >= outer.top &&
            own.bottom <= outer.bottom
          );
        },
        await screen.elementHandle(),
      );
      expect(within).toBe(true);
    }
    await screenshotBothThemes(page, 'sidebar-collection-nested');
  });
});
