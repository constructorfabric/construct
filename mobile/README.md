# Construct mobile

Construct on the phone: an Android app and an iPhone app that share their logic in Kotlin
Multiplatform. This folder holds the shared Gradle project.

## Layout

| Module           | What it holds                                                                               |
| ---------------- | ------------------------------------------------------------------------------------------- |
| `core/model`     | The types: conversation, message, fact, dashboard.                                          |
| `core/construct` | The one interface the screens use for chats, memory and the dashboard, and `FakeConstruct`. |
| `core/database`  | The phone's copy in SQLite (SQLDelight), behind the same interface. Sending says "not available yet". |

The calls and names follow the desktop app's `chat-protocol.ts`, `memory-protocol.ts` and
`dashboard-protocol.ts` in `app/src`. A screen talks to `Construct` and never knows where the data
comes from. `FakeConstruct` has seeded data and a canned streamed answer, for tests and previews.

The copy keeps the dashboard types and the fact values as JSON text, and enums by name. A change to
those types in `core/model` that old rows no longer fit needs a migration (`core/database`), like a
column change does.

Targets: Android (minimum API 26, set once in `gradle/libs.versions.toml`), `iosArm64` and
`iosSimulatorArm64`. Every version lives in `gradle/libs.versions.toml`.

## Build and check

You need a Mac with JDK 17 (`JAVA_HOME` set to it), the Android SDK (through `ANDROID_HOME` or
`sdk.dir` in an uncommitted `local.properties`) and Xcode. The first Kotlin/Native build downloads
its toolchain (about 1.6 GB).

```sh
cd mobile
./gradlew check                                           # tests, ktlint, detekt
./gradlew :core:construct:compileKotlinIosSimulatorArm64  # the iPhone simulator target
./gradlew :core:database:linkDebugFrameworkIosSimulatorArm64  # the copy store as an iPhone framework
./gradlew ktlintFormat                                    # fix ktlint findings
```
