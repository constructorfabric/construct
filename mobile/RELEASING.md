# Signed mobile releases

Use the Mobile release workflow on canonical `main`. It accepts one platform, the full SHA of
the current `main` commit, and explicit confirmation. Commit the intended release version and
build number to `main` before dispatching. The release stops if `main` advances before upload.

The desktop publish job and the mobile release jobs run only in repositories named by the
`RELEASE_REPOSITORIES` Actions variable: a JSON array of exact `owner/name` strings, such as
`["constructorfabric/construct"]`. Unset, it allows only the canonical repository. Use an array
and nothing else: the mobile guard rejects any other value, and the workflow condition would
match a plain string by substring. A malformed value fails those workflow runs. A listed repository
is treated as canonical: its own `main` is the release source, and it needs the environment below.

All signed releases (iPhone, Android and the desktop disk image) run in one GitHub Actions
environment, `release`. Allow deployments from the `main` branch and from `v*` tags only, so that
the credentials are not available to workflows of other branches. Keep the credentials as
environment secrets.

| Release | Secrets                                                                                                          | Variable                    |
| ------- | ---------------------------------------------------------------------------------------------------------------- | --------------------------- |
| iPhone  | `APP_STORE_CONNECT_KEY_P8_BASE64`, `APP_STORE_CONNECT_KEY_ID`, `APP_STORE_CONNECT_ISSUER_ID`                     | `TESTFLIGHT_INTERNAL_GROUP` |
| Android | `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEY_ALIAS`, `ANDROID_KEYSTORE_PASSWORD`, optional `ANDROID_KEY_PASSWORD`     |                             |
| Desktop | `MAC_CERTIFICATE_BASE64`, `MAC_CERTIFICATE_PASSWORD`, `APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD`, `APPLE_TEAM_ID` |                             |

`scripts/setup-release-environment.sh` creates the environment and sets these secrets from environment
variables and files; the variables are listed at the top of the script. Run it as an administrator of
the repository. It can be run again at any time to restore the environment, and `DRY_RUN=1` checks the
inputs and changes nothing.

Use an App Store Connect team key with access to the app, its build and upload history, and
the intended internal group. Set `TESTFLIGHT_INTERNAL_GROUP` to the exact group name and enable
automatic distribution of new builds to it. The workflow verifies the unsigned and signed
archive metadata against `iosApp/project.yml`. It checks the build number against App Store
Connect before signing and again before upload, then waits for processing and internal testing.
If verification fails after export, check App Store Connect before dispatching again.

For Android, commit the final application ID and a new version name and code in
`gradle.properties`. The Android publish job creates a pre-release from the checked-out commit.

The PR job tests release guards with fixture data. For a local iPhone build and upload, see
`build-ios.sh` in [the mobile README](README.md).
