import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import plist from 'plist';
import { describe, expect, it } from 'vitest';
import config from '../forge.config.mjs';
import { agentIdentifier, entitlementsFile, macInfoPlist } from '../mac-bundle.mjs';

const appDir = join(__dirname, '..');

function readPlist(path: string): Record<string, unknown> {
  return plist.parse(readFileSync(path, 'utf8')) as Record<string, unknown>;
}

/** The Info.plist of every packaged macOS app under `out/`; none on a system that did not package one. */
function packagedInfoPlists(): string[] {
  const out = join(appDir, 'out');
  if (!existsSync(out)) return [];
  return readdirSync(out)
    .filter((dir) => dir.includes('-darwin-'))
    .flatMap((dir) =>
      readdirSync(join(out, dir))
        .filter((name) => name.endsWith('.app'))
        .map((app) => join(out, dir, app, 'Contents', 'Info.plist')),
    )
    .filter((path) => existsSync(path));
}

describe('what macOS needs from the app', () => {
  it('asks to send Apple Events and explains why', () => {
    expect(macInfoPlist.NSAppleEventsUsageDescription.length).toBeGreaterThan(20);
  });

  it('signs with the Apple Events entitlement and nothing that weakens the Hardened Runtime', () => {
    expect(readPlist(join(appDir, entitlementsFile))).toEqual({
      'com.apple.security.automation.apple-events': true,
    });
  });

  it('names the agent like its LaunchAgent label', () => {
    const source = readFileSync(
      join(appDir, '..', 'agent', 'src', 'autostart', 'macos.rs'),
      'utf8',
    );
    expect(/pub const LABEL: &str = "([^"]+)";/.exec(source)?.[1]).toBe(agentIdentifier);
  });

  it('hands the usage description to the packager', () => {
    expect(config.packagerConfig?.extendInfo).toEqual(macInfoPlist);
  });

  it.skipIf(packagedInfoPlists().length === 0)(
    'carries the usage description in a packaged bundle',
    () => {
      for (const path of packagedInfoPlists()) {
        expect(readPlist(path)['NSAppleEventsUsageDescription']).toBe(
          macInfoPlist.NSAppleEventsUsageDescription,
        );
      }
    },
  );
});
