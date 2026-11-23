import { once } from 'node:events';
import jsQR from 'jsqr';
import type { Page } from '@playwright/test';
import { PHONE_LIST_CHANNEL, PHONE_STATUS_CHANNEL } from '../src/phone-protocol';
import { settingsReply } from './dashboard-shared';
import {
  expect,
  expectNoPolicyViolations,
  gate,
  openSettings,
  test,
  withStandIn,
} from './fixtures';
import { screenshotBothThemes } from './screenshots';

const payload = 'construct-pair://192.0.2.1:1234?v=1&fp=example&s=example';
const offer = { payload, code: '12345678', address: '192.0.2.1:1234', expires_in: 300 };
const first = { id: 'first', name: "Anna's phone", paired_at: 1735718400 };
const second = { id: 'second', name: 'Work phone', paired_at: 1735891200 };

function pairingAgent(extra: (line: string) => string | Promise<string> | undefined) {
  return (line: string): string | Promise<string> => {
    const reply = extra(line);
    if (reply !== undefined) return reply;
    if (line === 'phone-pairing-open') return JSON.stringify(offer);
    if (line === 'phone-pairing-close') return '{"ok":true}';
    if (line === 'phone-pairing-status') return '{"active":true}';
    if (line.startsWith('phone-list ')) return '{"phones":[],"next":null}';
    if (line === 'settings-get') return settingsReply(true);
    if (line === 'dashboard-setup-status') {
      return JSON.stringify({ state: 'idle', sessions: 0, text_bytes: 0, sent: 0, received: 0 });
    }
    return JSON.stringify({ status: 'running', version: '0.1.0', pid: 42, model: 'example' });
  };
}

async function openPairing(page: Page): Promise<void> {
  await openSettings(page, 'Model');
  await page.getByRole('button', { name: 'Pair a phone' }).click();
  await expect(page.getByRole('heading', { name: 'Pair a phone' })).toBeVisible();
  await expect(page.locator('#phone-pairing-code')).toHaveText(offer.code);
}

test('pairing shows the exact QR offer and each phone’s pairing time, and unpairs a phone', async ({
  session,
}) => {
  let phones = [first, second];
  let unpairRequest: string | undefined;
  await withStandIn(
    session,
    pairingAgent((line) => {
      if (line.startsWith('phone-list ')) return JSON.stringify({ phones, next: null });
      if (line.startsWith('phone-unpair ')) {
        unpairRequest = line;
        phones = [second];
        return '{"ok":true}';
      }
      return undefined;
    }),
    async (page) => {
      await openPairing(page);
      await expect(page.locator('#phone-pairing-address')).toHaveText(offer.address);
      const qr = page.locator('svg#phone-pairing-qr');
      const png = await qr.screenshot();
      const pixels = await page.evaluate(async (base64) => {
        const bytes = Uint8Array.from(atob(base64), (character) => character.charCodeAt(0));
        const image = await createImageBitmap(new Blob([bytes], { type: 'image/png' }));
        const canvas = new OffscreenCanvas(image.width, image.height);
        const context = canvas.getContext('2d');
        if (context === null) throw new Error('Canvas is unavailable');
        context.drawImage(image, 0, 0);
        const { data, width, height } = context.getImageData(0, 0, image.width, image.height);
        return { data: Array.from(data), width, height };
      }, png.toString('base64'));
      const decoded = jsQR(new Uint8ClampedArray(pixels.data), pixels.width, pixels.height);
      expect(decoded?.data).toBe(payload);
      const modules = 17 + 4 * (decoded?.version ?? 0);
      await expect(qr).toHaveAttribute('viewBox', `0 0 ${modules + 8} ${modules + 8}`);

      const rows = page.locator('#phone-pairing-list').getByRole('listitem');
      for (const phone of [first, second]) {
        const time = rows.filter({ hasText: phone.name }).locator('time');
        await expect(time).toHaveAttribute(
          'datetime',
          new Date(phone.paired_at * 1000).toISOString(),
        );
        const displayed = await page.evaluate(
          (seconds) => new Date(seconds * 1000).toLocaleString(),
          phone.paired_at,
        );
        await expect(time).toHaveText(`Paired ${displayed}`);
      }
      await screenshotBothThemes(page, 'phone-pairing');

      await page.getByRole('button', { name: "Unpair Anna's phone" }).click();
      await expect(rows.filter({ hasText: first.name })).toHaveCount(0);
      await expect(rows.filter({ hasText: second.name })).toBeVisible();
      expect(unpairRequest).toBe('phone-unpair {"id":"first","ack_only":true}');
    },
  );
});

test('the roster can reach and unpair a phone past the first 1024', async ({ session }) => {
  const phones = Array.from({ length: 1025 }, (_, index) => ({
    id: String(index).padStart(4, '0'),
    name: `Phone ${index + 1}`,
    paired_at: first.paired_at + index,
  }));
  const lastPhone = phones.at(-1);
  let unpaired: string | undefined;
  await withStandIn(
    session,
    pairingAgent((line) => {
      if (line.startsWith('phone-list ')) {
        const { after } = JSON.parse(line.slice('phone-list '.length)) as { after: string | null };
        const start = after === null ? 0 : Number(after) + 1;
        const rows = phones.slice(start, start + 8);
        return JSON.stringify({
          phones: rows,
          next: start + 8 < phones.length ? rows.at(-1)?.id : null,
        });
      }
      if (line.startsWith('phone-unpair ')) {
        const { id } = JSON.parse(line.slice('phone-unpair '.length)) as { id: string };
        unpaired = id;
        if (id === lastPhone?.id) phones.pop();
        return '{"ok":true}';
      }
      return undefined;
    }),
    async (page) => {
      await openPairing(page);
      const next = page.getByRole('button', { name: 'Next phones' });
      const previous = page.getByRole('button', { name: 'Previous phones' });
      const rows = page.locator('#phone-pairing-list').getByRole('listitem');
      await expect(previous).toBeDisabled();
      for (let start = 0; start < 1024; start += 128) {
        await expect(rows.first()).toContainText(`Phone ${start + 1}`);
        await next.click();
      }
      await expect(rows).toHaveCount(1);
      await expect(next).toBeDisabled();
      await page.getByRole('button', { name: 'Unpair Phone 1025' }).click();
      await expect(rows).toHaveCount(0);
      expect(unpaired).toBe('1024');
      await previous.click();
      await expect(rows).toHaveCount(128);
      await expect(rows.last()).toContainText('Phone 1024');
    },
  );
});

test('a used offer and an expired offer can each be replaced', async ({ session }) => {
  let offers = 0;
  let active = true;
  await withStandIn(
    session,
    pairingAgent((line) => {
      if (line === 'phone-pairing-open') {
        active = true;
        offers++;
        return JSON.stringify(
          offers === 1
            ? offer
            : {
                ...offer,
                code: offers === 2 ? '87654321' : '23456789',
                expires_in: offers === 2 ? 3 : 300,
              },
        );
      }
      if (line === 'phone-pairing-status') return JSON.stringify({ active });
      return undefined;
    }),
    async (page) => {
      await page.clock.install();
      await openPairing(page);
      const code = page.locator('#phone-pairing-code');
      const fresh = page.getByRole('button', { name: 'Get a new code' });
      const ended = page.getByText('This code has expired or is no longer valid.');
      active = false;
      await page.clock.fastForward(1000);
      await expect(ended).toBeVisible();
      await expect(code).toHaveCount(0);
      await fresh.click();
      await expect(code).toHaveText('87654321');
      await page.clock.fastForward(3000);
      await expect(ended).toBeVisible();
      await expect(code).toHaveCount(0);
      await fresh.click();
      await expect(code).toHaveText('23456789');
    },
  );
});

test('leaving the screen closes pairing while a roster reply is pending and reports failures', async ({
  session,
}) => {
  const roster = gate();
  let reading = false;
  let closes = 0;
  try {
    await withStandIn(
      session,
      pairingAgent((line) => {
        if (line.startsWith('phone-list ')) {
          reading = true;
          return roster.promise.then(() => '{"phones":[],"next":null}');
        }
        if (line === 'phone-pairing-close') {
          closes++;
          if (closes <= 2) return '{"error":"could not close the offer"}';
        }
        return undefined;
      }),
      async (page) => {
        await openPairing(page);
        await expect.poll(() => reading).toBe(true);
        await page.getByRole('button', { name: 'Status', exact: true }).click();
        await expect(page.getByText('Could not confirm that pairing ended.')).toBeVisible({
          timeout: 2000,
        });
        expect(closes).toBe(2);
        roster.release();
      },
    );
  } finally {
    roster.release();
  }
});

test('the offer and roster polls recover after an IPC rejection', async ({ session }) => {
  await withStandIn(
    session,
    pairingAgent(() => undefined),
    async (page, app) => {
      await app.evaluate(
        ({ ipcMain }, channels) => {
          let rejected = 0;
          for (const channel of channels) {
            ipcMain.removeHandler(channel);
            ipcMain.handleOnce(channel, () => {
              Reflect.set(globalThis, 'rejectedPairingPolls', ++rejected);
              throw new Error('test IPC rejection');
            });
          }
        },
        [PHONE_LIST_CHANNEL, PHONE_STATUS_CHANNEL],
      );
      await openPairing(page);
      await expect(page.getByText('Could not read the paired phones.')).toBeVisible();
      await expect
        .poll(() => app.evaluate(() => Reflect.get(globalThis, 'rejectedPairingPolls') as number))
        .toBe(2);
      await app.evaluate(
        ({ ipcMain }, channels) => {
          ipcMain.handle(channels.list, () => ({ ok: true, phones: [], next: null }));
          ipcMain.handle(channels.status, () => ({ ok: true, active: false }));
        },
        { list: PHONE_LIST_CHANNEL, status: PHONE_STATUS_CHANNEL },
      );
      await expect(page.getByText('This code has expired or is no longer valid.')).toBeVisible();
      await expect(page.getByText('No phones paired yet.')).toBeVisible({ timeout: 8000 });
      await expect(page.getByText('Could not read the paired phones.')).toHaveCount(0);
    },
  );
});

test('reloading closes the old renderer’s offer before it can open a new one', async ({
  session,
}) => {
  const closing = gate();
  let opens = 0;
  let closes = 0;
  await withStandIn(
    session,
    pairingAgent((line) => {
      if (line === 'phone-pairing-open') opens++;
      if (line === 'phone-pairing-close') {
        closes++;
        return closing.promise.then(() => '{"ok":true}');
      }
      return undefined;
    }),
    async (page, app) => {
      try {
        await openPairing(page);
        const loaded = page.waitForEvent('load');
        await app.evaluate(({ BrowserWindow }) =>
          BrowserWindow.getAllWindows()[0]?.webContents.reload(),
        );
        await loaded;
        await expect.poll(() => closes).toBe(1);
        await expect(page.locator('#phone-pairing-code')).toHaveCount(0);
        const early = await page.evaluate(() => window.constructAgent.openPhonePairing());
        expect(early.ok).toBe(false);
        expect(opens).toBe(1);
        closing.release();
        await expect
          .poll(() =>
            page.evaluate(async () => (await window.constructAgent.openPhonePairing()).ok),
          )
          .toBe(true);
        expect(opens).toBe(2);
      } finally {
        closing.release();
      }
    },
  );
});

test('renderer exit closes its pairing offer while the window remains open', async ({
  session,
}) => {
  let closes = 0;
  await withStandIn(
    session,
    pairingAgent((line) => {
      if (line === 'phone-pairing-close') closes++;
      return undefined;
    }),
    async (page, app) => {
      await openPairing(page);
      await app.evaluate(({ BrowserWindow }) => {
        setImmediate(() => BrowserWindow.getAllWindows()[0]?.webContents.forcefullyCrashRenderer());
      });
      await expect.poll(() => closes).toBe(1);
      expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length)).toBe(
        1,
      );
    },
  );
});

for (const mode of ['window close', 'app quit'] as const) {
  test(`${mode} waits for cleanup and rejects a new offer during shutdown`, async ({ session }) => {
    const closing = gate();
    let opens = 0;
    let closes = 0;
    await withStandIn(
      session,
      pairingAgent((line) => {
        if (line === 'phone-pairing-open') opens++;
        if (line === 'phone-pairing-close') {
          closes++;
          return closing.promise.then(() => '{"ok":true}');
        }
        return undefined;
      }),
      async (page, app) => {
        try {
          await openPairing(page);
          await expectNoPolicyViolations(page);
          const exit = mode === 'app quit' ? once(app.process(), 'exit') : undefined;
          const closed = mode === 'window close' ? page.waitForEvent('close') : undefined;
          await app.evaluate(({ app, BrowserWindow }, action) => {
            setImmediate(() => {
              if (action === 'app quit') app.quit();
              else BrowserWindow.getAllWindows()[0]?.close();
            });
          }, mode);
          await expect.poll(() => closes).toBe(1);
          expect(page.isClosed()).toBe(false);
          const late = await page.evaluate(() => window.constructAgent.openPhonePairing());
          expect(late).toEqual({ ok: false, error: 'The pairing window is closing.' });
          expect(opens).toBe(1);
          closing.release();
          if (exit !== undefined) expect(await exit).toEqual([0, null]);
          await closed;
        } finally {
          closing.release();
        }
      },
      { allowWindowClose: true },
    );
  });
}

test('a CSP violation during shutdown fails the stand-in check after the window closes', async ({
  session,
}) => {
  const closing = gate();
  let closes = 0;
  await expect(
    withStandIn(
      session,
      pairingAgent((line) => {
        if (line === 'phone-pairing-close') {
          closes++;
          return closing.promise.then(() => '{"ok":true}');
        }
        return undefined;
      }),
      async (page, app) => {
        try {
          await openPairing(page);
          const closed = page.waitForEvent('close');
          await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]?.close());
          await expect.poll(() => closes).toBe(1);
          await page.evaluate(
            () =>
              new Promise<void>((resolve) => {
                document.addEventListener('securitypolicyviolation', () => resolve(), {
                  once: true,
                });
                const script = document.createElement('script');
                script.textContent = 'window.inlineScriptRan = true';
                document.head.append(script);
              }),
          );
          closing.release();
          await closed;
        } finally {
          closing.release();
        }
      },
      { allowWindowClose: true },
    ),
  ).rejects.toThrow(/Content security policy violations[\s\S]*script-src/);
});
