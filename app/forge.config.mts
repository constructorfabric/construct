import { FuseV1Options, FuseVersion } from '@electron/fuses';
import { MakerDeb } from '@electron-forge/maker-deb';
import { MakerDMG } from '@electron-forge/maker-dmg';
import { MakerSquirrel } from '@electron-forge/maker-squirrel';
import { FusesPlugin } from '@electron-forge/plugin-fuses';
import { VitePlugin } from '@electron-forge/plugin-vite';
import path from 'node:path';
import type { ForgeConfig } from '@electron-forge/shared-types';
import { entitlementsFile, macInfoPlist } from './mac-bundle.mjs';
import { agentName } from './src/agent-service';

type OsxSignOptions = Exclude<NonNullable<NonNullable<ForgeConfig['packagerConfig']>['osxSign']>, boolean>;

// The signed release job (.github/workflows/release.yml) imports the Developer ID certificate into a keychain
// of its own with scripts/macos-import-certificate.sh and sets these. Without them (a pull request, a
// developer's Mac) the app gets an ad hoc signature.
const identity = process.env.MAC_SIGN_IDENTITY;
const keychain = process.env.MAC_SIGN_KEYCHAIN;

// Signed with the hardened runtime and a secure timestamp (the packager's defaults), as the notarization
// requires. Electron and its helpers get the packager's default entitlements; the agent, which is a program of
// its own inside the app, gets only the one that lets it send Apple Events.
const developerIdSign: OsxSignOptions = {
  identity,
  keychain,
  // The packager's default is to go on after a failed signature with a warning that Forge does not show, which
  // hides the cause. Fail at the signature instead.
  continueOnError: false,
  optionsForFile: (file) => ({
    hardenedRuntime: true,
    ...(path.basename(file) === agentName ? { entitlements: path.resolve(entitlementsFile) } : {}),
  }),
};

// Ad hoc signature of the whole bundle, made after the fuses are flipped: an unsigned arm64 app does not start,
// and the packager's own signature does not survive the fuses. It has no hardened runtime: the library
// validation of that needs a Team ID, which an ad hoc signature lacks, so dyld would refuse Electron Framework.
// A Developer ID has a Team ID, so the signed build has the hardened runtime.
const adHocSign: OsxSignOptions = {
  identity: '-',
  identityValidation: false,
  optionsForFile: () => ({ hardenedRuntime: false }),
};

// Notarization (and stapling of the ticket to the app) only for a signed build, with the login of the release
// environment, read by name.
const appleId = process.env.APPLE_ID;
const appleIdPassword = process.env.APPLE_APP_SPECIFIC_PASSWORD;
const teamId = process.env.APPLE_TEAM_ID;
const osxNotarize =
  identity && appleId && appleIdPassword && teamId ? { appleId, appleIdPassword, teamId } : undefined;

const config: ForgeConfig = {
  packagerConfig: {
    asar: true,
    appBundleId: 'cf.construct.browser',
    ...(process.platform === 'darwin' ? { osxSign: identity ? developerIdSign : adHocSign, osxNotarize } : {}),
    // The name of the program in the package: /usr/bin/construct-browser, not the product name with a space.
    executableName: 'construct-browser',
    // Only macOS reads these keys; the other systems ignore them.
    extendInfo: macInfoPlist,
    // The agent is its own program: it goes next to the app, outside the asar archive,
    // and the app finds it in `process.resourcesPath`.
    extraResource: [`../target/release/${agentName}`],
  },
  rebuildConfig: {},
  makers: [
    new MakerSquirrel({
      authors: 'Constructor Fabric Foundation',
      setupExe: 'construct-browser-x64-Setup.exe',
    }),
    new MakerDMG({}),
    new MakerDeb({ options: { name: 'construct-browser', bin: 'construct-browser' } }),
  ],
  plugins: [
    new VitePlugin({
      build: [
        {
          entry: 'src/main.ts',
          config: 'vite.main.config.mts',
          target: 'main',
        },
        {
          entry: 'src/preload.ts',
          config: 'vite.preload.config.mts',
          target: 'preload',
        },
      ],
      renderer: [
        {
          name: 'main_window',
          config: 'vite.renderer.config.mts',
        },
      ],
    }),
    // Fuses are used to enable/disable various Electron functionality
    // at package time, before code signing the application
    new FusesPlugin({
      version: FuseVersion.V1,
      [FuseV1Options.RunAsNode]: false,
      [FuseV1Options.EnableCookieEncryption]: true,
      [FuseV1Options.EnableNodeOptionsEnvironmentVariable]: false,
      [FuseV1Options.EnableNodeCliInspectArguments]: false,
      [FuseV1Options.EnableEmbeddedAsarIntegrityValidation]: true,
      [FuseV1Options.OnlyLoadAppFromAsar]: true,
    }),
  ],
};

export default config;
