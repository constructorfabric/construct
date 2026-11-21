# Construct mobile

Construct on the phone: an Android app and an iPhone app that share their logic in Kotlin
Multiplatform. This folder holds the shared Gradle project.

## Layout

| Module           | What it holds                                                                               |
| ---------------- | ------------------------------------------------------------------------------------------- |
| `core/model`     | The types: conversation, message, fact, dashboard.                                          |
| `core/construct` | The one interface the screens use for chats, memory and the dashboard, and `FakeConstruct`. |
| `core/database`  | The phone's copy in SQLite (SQLDelight), behind the same interface. Sending says "not available yet". |
| `core/designsystem` | The tokens both apps share, as plain numbers: the colour roles of the desktop app's kit in light and dark, and a scale of spacing steps. Text sizes, radii and control sizes belong to each app's own theme. |
| `feature/template`  | An empty feature to copy: a state, a state holder (plain common Kotlin that both apps watch), an Android screen that shows the state and loads nothing, and a test with `FakeConstruct`. |
| `feature/memory`    | The first feature made from the template: the person's facts as a plain list. |
| `androidApp`        | The Android app. `Composition.kt` is the one place its objects are built (the copy in the app; `FakeConstruct` would replace it there), `ConstructTheme.kt` maps the shared colours to the Compose theme and holds the Android text sizes and radii. Debug builds fill an empty copy with the sample data; release builds do not. |

The state holders wrap the one-shot reads of `Construct`: a fact changed elsewhere shows after `refresh()`, not by itself. A read that is still running is dropped when a new one starts. A `Construct` reports a failed read as `Outcome.Failed` and does not throw; run a holder's scope and call `refresh()` on the main thread.

The calls and names follow the desktop app's `chat-protocol.ts`, `memory-protocol.ts` and
`dashboard-protocol.ts` in `app/src`. A screen talks to `Construct` and never knows where the data
comes from. `FakeConstruct` has seeded data and a canned streamed answer, for tests and previews.

The copy keeps the dashboard types and the fact values as JSON text, and enums by name. Removing
a field needs no migration; adding a required field or renaming a field or a constant does
(`core/database`), like a column change.

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
./gradlew :feature:memory:compileKotlinIosSimulatorArm64      # a feature for the iPhone simulator
./gradlew :androidApp:assembleDebug                      # the Android app
./gradlew ktlintFormat                                    # fix ktlint findings
```
