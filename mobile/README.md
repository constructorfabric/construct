# Construct mobile

Construct on the phone: an Android app and an iPhone app that share their logic in Kotlin
Multiplatform. This folder holds the shared Gradle project.

## Layout

| Module               | What it holds                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| -------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `core/model`         | The types: conversation, message, fact, dashboard.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `core/construct`     | The one interface the screens use for chats, memory and the dashboard, and `FakeConstruct`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `core/database`      | The phone's copy in SQLite (SQLDelight) and `PhoneConstruct`, which runs chat with the copy as its history and facts.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `core/designsystem`  | The tokens both apps share, as plain numbers: the colour roles of the desktop app's kit in light and dark, and a scale of spacing steps. Text sizes, radii and control sizes belong to each app's own theme.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `core/connection`    | Connecting to the computer: the pairing offer (a QR payload or a typed code), the line protocol, the connection state and the stored pairing. The Rust client is reached through `core/chat`; the pairing secret is kept by `core/keys`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `core/sync`          | Copying from the computer over the paired connection: facts, chats and the dashboard's numbers in three steps with counts, a check that copies only what changed, each sync applied whole or not at all. `SyncEngine` has no screen; `feature/status` holds the state.                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `core/chat`          | The Rust chat core as Kotlin sees it: streaming answers, testing a model setting, and building the fixed request fixture. Android calls it through JNA and iOS uses cinterop.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `core/keys`          | `ModelSettingsStore` keeps the model address, model name and API key together in the Keychain (iPhone) or under a Keystore key (Android). Its contract tests also run against `FakeModelSettingsStore`. See "The model setting and its key".                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `feature/template`   | An empty feature to copy: a state, a state holder (plain common Kotlin that both apps watch), an Android screen that shows the state and loads nothing, and a test with `FakeConstruct`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `feature/memory`     | The first feature made from the template: the person's facts as a plain list.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `feature/settings`   | The model setup of the Settings page, made from the template: a state, a state holder (read, save, test, remove the key) and the Android screen with its form row and message line.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `feature/glasshouse` | What both phones share about the Glasshouse page, the page that opens a web address as on the computer: the state, its state holder, the rule for a typed address, the kinds of problem and the User-Agent. Android adds the address bar, the screen and `GlasshouseWebView`, the phone's web view set up for the page; the iPhone's web view comes with its page. See "The Glasshouse page".                                                                                                                                                                                                                                                                                                                               |
| `feature/dashboard`  | The Dashboard page of the phone, made from the template: the state (the computer's document, each widget with the numbers the copy holds for the day or the week the person is on, and the movement between days and weeks) and its state holder. The iPhone draws it; the Android screen in the module only shows that the holder reaches Android. See "The Dashboard page".                                                                                                                                                                                                                                                                                                                                               |
| `shared`             | The one Kotlin framework the iPhone app links (`ConstructShared`): it re-exports the modules above, adds `UiScope` (a main-thread scope Swift can create and close) and uses SKIE, which shows Kotlin flows to Swift as `AsyncSequence`. Its analytics upload is off.                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `androidApp`         | The Android app. `Composition.kt` is the one place its objects are built (the copy in the app; `FakeConstruct` would replace it there), `ConstructTheme.kt` maps the shared colours to the Compose theme and holds the Android text sizes and radii. Debug builds fill an empty copy with the sample data; release builds do not. `AppFrame.kt` is the frame of every page (top bar with the title, a connection line, the navigation bar); a page without a feature is a `PlaceholderPage` until a feature module takes it over.                                                                                                                                                                                           |
| `iosApp`             | The iPhone app in SwiftUI. `project.yml` generates the Xcode project (not committed); Swift files under `App/`, `Core/` and `Features/` are picked up by folder. `Composition.swift` is the one place its objects are built, `Core/FlowState.swift` is the one adapter every feature uses to watch a state holder, `Core/ConstructTheme.swift` maps the shared colours to SwiftUI and holds the iPhone text sizes, radii and control sizes. `App/AppFrame.swift` is the frame of every page (the tab bar and a navigation bar with the inline title; the sync's notice is the first row of a page, see "Sync the copy from the computer"); every page has a feature folder. The tab icons are Lucide, in the asset catalog. |

The Memory holder watches the stream `Construct.memoryFacts`, so a fact changed in the copy shows by itself, and `refresh()` starts the watch again. The Chat holder also reads its list again when a sync changes the chats (`Construct.conversationChanges`). The other state holders wrap the one-shot reads of `Construct`: a change shows after `refresh()`, not by itself. A read or a watch that is still running is dropped when a new one starts. A `Construct` reports a failed read as `Outcome.Failed` and does not throw (the stream gives it as an item and goes on); run a holder's scope and call `refresh()` on the main thread.

The calls and names follow the desktop app's `chat-protocol.ts`, `memory-protocol.ts` and
`dashboard-protocol.ts` in `app/src`. A screen talks to `Construct` and never knows where the data
comes from. `FakeConstruct` has seeded data and a canned streamed answer, for tests and previews.

The copy keeps the dashboard types and the fact values as JSON text, and enums by name. Removing
a field needs no migration; adding a required field or renaming a field or a constant does
(`core/database`), like a column change.

Targets: Android (minimum API 26, set once in `gradle/libs.versions.toml`), `iosArm64` and
`iosSimulatorArm64`. Every version lives in `gradle/libs.versions.toml`.

## The chat core

The chat is one Rust library, the `chat-core` crate, shared with the desktop agent. The `chat-bridge`
crate puts a plain C interface on it (`chat-bridge/include/construct_chat.h`), and the shared Kotlin
code calls that through `core/chat`:

- `ChatCore.fixedRequestBody(toolsFile)` builds the request body of a fixed test conversation. The
  bytes are the ones the provider's prompt cache sees, and equal the stored files of
  `chat-core/tests/golden`.
- `ChatCore.testModel(baseUrl, model, apiKey)` tests a model setting and returns `Works` or
  `Failed(reason)`, the reason in the desktop app's words. The address check of the core refuses a
  plain `http` address that is not on `localhost` or `127.0.0.1`. The key is an argument only: it is in
  no log and in no returned text.
- `PhoneConstruct` takes the model setting from the phone's settings store and passes a snapshot of
  the saved history and facts to Rust. The native answer sends desktop chat events and commits
  records to SQLite as the answer runs. Only `recall_facts` is offered on the phone. Chats copied
  from a computer are read-only, and deleting all chats leaves them in place.

Why a C interface: Kotlin/Native cannot load the Kotlin bindings that Rust bridge generators write,
and a C header is read by cinterop as it is. Android calls the same functions by name through JNA
(no glue code to write for JNI; the one exception is the JNI function that gives Android's certificate
check the JVM, see `https` on Android below). A streamed answer calls back through a C function
pointer with a context, and a separate stop token can be cancelled while the answer is running.

`scripts/build-core.sh [ios|android|all]` builds the core once for each target into a folder outside
the repository, `CONSTRUCT_CORE_BUILD_DIR` (default `~/Library/Caches/construct-chat-core`), where the
cargo cache is too unless `CARGO_TARGET_DIR` is set:

```text
<dir>/ios/device/libconstruct_chat_bridge.a        iPhone device (aarch64-apple-ios)
<dir>/ios/simulator/libconstruct_chat_bridge.a     iPhone Simulator (aarch64-apple-ios-sim)
<dir>/android/<abi>/libconstruct_chat_bridge.so    arm64-v8a and x86_64
```

The Gradle tasks `buildChatCoreIos` and `buildChatCoreAndroid` run the script, and the iPhone
framework and the Android app depend on them, so Xcode and Gradle builds build the core first. Their
inputs are the Rust sources, the phone tool file, `Cargo.lock`, the script and the two files it reads the platform versions
from, and their outputs are the libraries, so a
build with unchanged sources does not start cargo. Nothing compiled is committed.

What a Mac needs for the Rust part, in addition to the list below:

- `rustup` and the Rust toolchain of `rust-toolchain.toml`, with the four targets:
  `rustup target add aarch64-apple-ios aarch64-apple-ios-sim aarch64-linux-android x86_64-linux-android`
- `cargo-ndk`, which builds a Rust library for each Android ABI with the NDK's compiler (the bridge itself has no generator): `cargo install cargo-ndk`
- The Android NDK, any recent release (27.2 was proven): `sdkmanager 'ndk;27.2.12479018'`. The script takes
  `ANDROID_NDK_HOME`, or else the newest one under `$ANDROID_HOME/ndk`
- `cmake`, which the build script of the TLS library may ask for: `brew install cmake`
- Xcode, the app (the command line tools carry no iPhone SDK), as in the list below

The first build compiles every crate the core uses for each of the four targets and takes a quarter
of an hour on an Apple Silicon laptop; a second one with nothing changed does nothing. `./gradlew :androidApp:connectedDebugAndroidTest` runs the chat core's tests on a running emulator,
by hand: the workflow only compiles them, because it starts no emulator. `./gradlew :core:chat:iosSimulatorArm64Test` runs them in the iPhone Simulator, and the workflow runs that one.

The Android minimum API level and the iPhone deployment target of the Rust build are read from
`gradle/libs.versions.toml` and `iosApp/project.yml`, and both files are inputs of the Gradle tasks.
`rustSources` in `build.gradle.kts` and the paths of the workflow list the crates the library is built
from. The script stops when `cargo metadata` shows other crates than `chat-bridge`, `chat-core` and
`connectors` (the list it keeps), so a new path dependency cannot leave those two lists stale.

`https` on Android: the TLS stack checks certificates through Android's own trust store, which it
reaches through the JVM. The app calls `initChatCore(context)` (`core/chat`, in `Application.onCreate`)
once, which hands the context to the Rust library through one JNI function. If the init fails, or the
library does not load, the app logs it and goes on, since the memory screens do not need the core. The
Kotlin part of the check, `org.rustls:rustls-platform-verifier`, is an AAR that its authors publish as a Maven repository
on GitHub (`settings.gradle.kts` limits that repository to its own group). Its version is read from
`Cargo.lock`, so it follows the crate. A call made before a successful init
answers "unreachable" for an `https` address. If the library itself did not load, every call throws
`UnsatisfiedLinkError`: the library is built for `arm64-v8a` and `x86_64` only, so the app keeps only
those two (`abiFilters` in `androidApp/build.gradle.kts`, which also drops the 32-bit libraries of JNA) and a
32-bit device cannot install it.

The first `https` call of a process was slow on the emulator: 10 s, and once more than the core's
15 s deadline, so it came back as `timeout`; the next calls took under half a second. The emulator has
no DNS, and it is not known whether a real phone has the stall. Check it on a device before the chat
depends on it.
`AndroidTlsTest` shows the system check at work (and that `ConstructApp` started it): a local TLS
server with a certificate no phone trusts is refused with a TLS alert. The Gradle build stops when
`Cargo.lock` holds two copies of the verifier. No test reaches a real provider.

## The model setting and its key

Computer pairing uses a separate secure record in `core/keys`. See [Connect to a computer](#connect-to-a-computer).

`core/keys` keeps what the person typed on the Settings page: the address of the model, its name and the
API key. `ModelSettingsStore` is the one interface; a screen's state holder reads and writes it and the
screen never sees it. A setting read back says only that a key is set (`ModelSettings.keyIsSet`); the whole
key comes out with its provider through `readProvider()` for a call to the model and is never part of a state, a log line, an
error text or a copy. Every store call waits for the disk: the holder runs it off the main thread. A call
that fails throws `ModelSettingsException` with a fixed plain reason.

- Android: the address, model and key sit in one file under
  `noBackupFilesDir`, which the system never backs up, encrypted with AES-GCM under a 256-bit Keystore key
  that cannot be taken out of the Keystore. `res/xml/backup_rules.xml` and `data_extraction_rules.xml`
  exclude the key file and the preferences too, should `allowBackup` ever be turned on (`BackupRulesTest`
  reads them). A key that cannot be opened (a restored phone has no Keystore key) is reported and replaced
  by saving a new one.
- iPhone: the address, model and key form one generic-password Keychain item with
  `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` and `kSecAttrSynchronizable` false, so it is in no backup and not
  in iCloud Keychain. Saving replaces that item in one operation.

On upgrade, the existing address and model remain available from preferences until the next save.
Enter the key again to bind it to the selected provider. Earlier releases stored keys separately
from their addresses, so the app cannot establish which provider an old key belongs to. The first
settings access deletes the old credential from secure storage without opening it.
A saved key stays with its provider when the model changes. If the provider record cannot be read,
Settings keeps the form available for an explicit replacement.

Tests: `ModelSettingsStoreContract` (`core/keys`, common) is what every store does; the fake, the shared
store over memory and the encrypted file (JVM, with an AES key of the JVM in place of the Keystore key) pass it.
On the phone, `ModelSetupTest` (`./gradlew :androidApp:connectedDebugAndroidTest`, an emulator) runs the real
Settings page against a fake provider on a local port. `iosApp/Tests/ModelSettingsStoreTests.swift` runs the
iPhone store over the real Keychain; the Keychain exists only inside the app, so it is an app test, not a
Kotlin one. `scripts/check-key-not-leaked.sh android|ios` saves a made-up key through the app, proves its
search by planting the key in a file, and then looks for the key in the app's files and in the log.
It uses a separate Android app ID or the shared iOS Simulator (see [The test Simulator](#the-test-simulator)).

## The Glasshouse page

`feature/glasshouse` is the part of the page that does not depend on a web view; the KDoc of
`GlasshouseHolder` says how the two talk. What a phone's page must do:

- Take the start address from the app's build configuration, not from the Settings page or `core/keys`.
  There is no built-in one: the computer's is a plain `http` address, which the phones block. With none,
  the page says so and the person types an address.
- Call everything of the holder on the main thread, and `webViewStarted()` once for each web view it
  makes, before it collects `commands`. Stop the old web view's collector first (cancel its coroutine or
  task): one that is still waiting would take the new view's first command, and one cancelled just as a
  command reached it loses that command. A collector that starts again after the old one stopped, as a
  screen that redraws does, continues from what is queued; a second `webViewStarted()` reopens the page
  and clears Back and Forward.
- Cancel every link, redirect and new window that `GlasshouseAddress.isWeb` refuses.
- Build the web view's address from the one in `GlasshouseCommand.Open`, which is kept as typed. Where the
  system cannot (the iPhone's `URL(string:)` gives nil), encode it for the system, and when that fails too
  report `pageFailed` with `Other`.

### The Glasshouse page on iPhone

`RootView` makes one `GlasshouseModel` (`iosApp/Features/Glasshouse`) when the app starts and keeps it, so a page that
SwiftUI draws again shows the same web view. The model holds the `GlasshouseHolder` and the one `WKWebView`, which it
makes when the person first opens the page. The page, what was typed in it and its history stay while the person is on
another page of the app, and a page that was loading keeps loading. The page has no title bar and no connection line:
its address bar takes their place.

- **Address bar and toolbar.** One rounded bar under the status area with the lock (only for an `https` page that
  opened), the host and reload. Tapped, it shows the whole address selected, with "Cancel" beside it, and the keyboard's
  Go key opens it. A refused address stays in the bar and the message is under it. Back and forward are above the tab
  bar, and a swipe from the edge of the screen goes back and forward too.
- **Start address.** `GLASSHOUSE_START_ADDRESS` in `iosApp/project.yml` puts it into the build: the Info.plist key
  `ConstructGlasshouseStartAddress`, which `iosApp/GlasshouseInfo.plist` adds to the generated Info.plist (Xcode adds only
  the keys it knows by itself). It is empty, so the page says there is none and the person types an address. It is not a
  setting of the app. To build for an environment of your own without changing the file, give
  `GLASSHOUSE_START_ADDRESS=https://...` in the environment of `scripts/build-ios.sh`: it is passed to Xcode as a build
  setting, for the Simulator build and for the archive. A debug build takes `CONSTRUCT_GLASSHOUSE_START_ADDRESS` from its
  environment instead, for the tests.
- **User-Agent.** `applicationNameForUserAgent` is `Construct/<version>`, where `<version>` is the app's
  `CFBundleShortVersionString`, so the web view adds it last to its own User-Agent for the page and for everything the page
  asks for, from the first request on.
- **Cookies.** The web view uses `WKWebsiteDataStore.default()`: the app's own persistent store, one set for every page,
  and not the phone's browser's. A cookie that has an end date is still there after the app restarts; one with none
  (a session cookie) is the page's to lose when the app ends, as in the phone's own browser.
- **What opens.** Only web addresses: a link, a redirect or a new window to any other kind of address does nothing and
  starts no other app, and a window a page asks for opens in the same page. A file the web view cannot show, or the
  server marks as an attachment, is not downloaded (see Restricted mode). The app never answers a certificate challenge, so a certificate the system does not trust is never
  accepted. Plain `http` is the phone's own default: a site with a name is refused (App Transport Security, and the
  Info.plist has no exception), and the page says it cannot open. An address that is an IP address or a name with no dot
  is not covered by that rule, as in the phone's own browser.
- **Permissions.** A page that asks for the camera or the microphone is refused in code, with no prompt, and so is the
  motion and orientation request. A request for the location has no call to refuse it in (the system asks only when the
  app has a usage text, and it has none, so the request would wait for ever): `GlasshouseRefusals` is a script that runs
  first in every frame and answers it with "permission denied". The app has no usage text for the microphone, the
  location or the photos, so the system never asks for them; the camera and local network texts are there for pairing
  with a computer.
- **Problems.** The system's error codes become the kinds of problem in `GlasshouseProblems`: a name that does not
  resolve is "not found", a connection that fails is "refused", a certificate the system does not trust is
  "certificate", and a page asked for with no network is "offline" at once: the app asks the system whether there is a
  network before it opens or reloads a page. A web view whose process stopped says "stopped working", and Reload makes it
  start a new one. A page that fails before it shows leaves the page before it in the web view, so back returns to that
  page.
- **Restricted mode.** The page is always in it, from the first moment, and nothing in the app switches it off. A small
  lock and "Restricted mode" under the address bar say so. It takes away what the system lets an app take away, by the
  system's basic way and no more; it does not look for clipboard managers or other tricks, and it does not stop the
  person dragging text out.
  - *Screenshots.* Apple has no function that keeps a page out of a screenshot, so the page says so before the
    environment opens: one line, "A screenshot of this page cannot be blocked on this iPhone.", with "OK", and the page
    opens at once under it. The one known way, showing the page inside a secure text field, is not an Apple function and
    is reported to fail on iOS 18 and later; it was tried in the Simulator and is not relied on (the PR that added this
    mode says what it showed). `GlasshouseScreenshot.canBeBlocked` is the switch that would turn the line off if a way
    that can be relied on is found.
  - *Recording and mirroring.* While the screen is recorded or shown on another screen (`UIScreen.isCaptured`), the
    page is covered by "The page is hidden while the screen is recorded or shown on another screen." and the web view
    is hidden from accessibility too. It is back when the recording ends.
  - *Touch menu.* `GlasshouseWebView` answers `canPerformAction` with no for "Copy", "Cut" and "Share", so the menu of
    selected text and of a text field has none of them, and a text field keeps "Paste". WebKit's "Copy Link with
    Highlight" is removed by `buildMenu(with:)` from iOS 26; it has no earlier way, so it stays on iOS 16 to 25. The
    web view answers the long touch menu of a link or a picture with none, so "Copy Image", "Save to Photos" and "Share"
    of a picture do not come up. Developer tools of the page (`isInspectable`) are off.
  - *Clipboard.* The page's own commands (a button that runs `navigator.clipboard.writeText`) still copy, and the person
    can paste inside the page. `GlasshouseClipboard` reads what the pasteboard holds when the page is shown, and puts
    it back when the page is left: the app goes to the background or the person goes to another page of the app. If
    the pasteboard held nothing, it is emptied. A change the app did not make, such as a copy in another app, is left
    alone. The system lets an app read only what it put on the pasteboard itself, or what the person allowed it to
    read, so when something from another app was there, the phone may ask "Allow Paste" when the page is shown; if that
    is refused or the content cannot be read, only the page's copy is removed and nothing can be put back. Something
    copied on another device while the page is open can be replaced the same way. The app switcher has no cover.
  - *Download, print and share.* A link with `download`, a file the server sends as an attachment and a file type the web
    view cannot show are cancelled; `window.print` and `navigator.share` are replaced by a script that runs first in
    every frame, and the share's promise is rejected with `NotAllowedError` as if the person said no. Each says in one
    line for a few seconds that it is blocked in restricted mode.

The page tests (`UITests/GlasshouseTests`) open real pages from a test server on the Mac over `https`. The app has no
way to trust a certificate and no code for the tests but three debug Simulator hooks (`GlasshouseReachability`, which
lets a test say the phone has no network, `--glasshouse-forget-data`, which starts a test with no cookies, and
`CONSTRUCT_GLASSHOUSE_TEST_PASTEBOARD`, which puts a text on the pasteboard as the app starts, as what the person copied
before); a release build has none of them. `scripts/test-glasshouse-ios.py` makes two certificates with `openssl`, puts the first one into the
trust store of the shared test Simulator (`simctl keychain add-root-cert`, see [The test Simulator](#the-test-simulator)), starts
`scripts/glasshouse_test_server.py` with both, runs the tests, and takes a screenshot of each state in both themes. The
second certificate is trusted by nothing and stands for a site that cannot be trusted.

The restricted mode tests are in `UITests/GlasshouseRestrictedTests`, and `iosApp/Tests` has unit tests of the touch menu
answers, the page's script (run in a real web view), the clipboard (on a fake pasteboard, and one round trip on the real
one), and the state of the screen. The test server has a page for them (`/restricted`), and the runner sets and reads
the Simulator's pasteboard for the clipboard tests, from outside the app, as another app would. Two things cannot be
tested in a Simulator: the recording cover (a Simulator cannot be made to report a recorded screen, so it is a unit
test of the state) and what the pasteboard holds that another app put there (the app cannot read it in a Simulator).
Both are checked on a real iPhone. `--only <Class>/<test>` runs one test, and `--only <Class>` a class.

```sh
cd iosApp && xcodegen generate && cd ..
xcodebuild -project iosApp/Construct.xcodeproj -scheme Construct -derivedDataPath <build folder> \
 -destination 'generic/platform=iOS Simulator' CODE_SIGNING_ALLOWED=YES CODE_SIGN_IDENTITY=- build-for-testing
python3 scripts/test-glasshouse-ios.py --ios-build <build folder> \
 --ios-device-type com.apple.CoreSimulator.SimDeviceType.iPhone-17 \
 --ios-runtime com.apple.CoreSimulator.SimRuntime.iOS-27-2 --screenshots <folder>
```

### The Glasshouse page on Android

`GlasshouseViewModel` (in `androidApp`) makes the holder and the one `GlasshouseWebView` when the person first opens
the page, and keeps both for as long as the activity's screen lives. The screen takes the web view out of its frame when
it leaves and puts it back when it returns, so the page, what was typed in it and its history stay while the person is
on another page of the app; a page that was loading keeps loading. The page has no title bar and no connection line:
its address bar takes their place.

- **Address bar.** One rounded bar with the lock (only for an `https` page that opened), the host and reload. Tapped,
  it shows the whole address and a clear button, and the keyboard's Go key opens it. A refused address stays in the bar
  and the message is under it. The system back goes back in the page while it can.
- **Start address.** `-PconstructGlasshouseStartAddress=<https address>` (`gradle.properties`) puts it into the build.
  It is empty, so the page says there is none and the person types an address. It is not a setting of the app.
- **User-Agent.** `Construct/<version>` goes last on the web view's own User-Agent, where `<version>` is the app's
  `versionName`; it is set before the first load, so the page, its scripts and its pictures all carry it.
- **Cookies.** The web views of an app share one `CookieManager` jar, kept in the app's own data, so it is not the
  phone's browser's. `flushCookies` writes it to the disk after each page and when the activity stops, so a sign-in
  survives the app being closed.
- **What opens.** Only web addresses: a link, a redirect or a new window to any other kind of address does nothing and
  starts no other app, and a window a page asks for opens in the same page. A certificate the system does not trust is
  never accepted. Plain `http` stays blocked by the app's network settings and shows the "cannot open" message.
- **Permissions.** A page that asks for the camera, the microphone, the location or anything else is refused, with no
  prompt. The manifest is unchanged by the page: the camera and local network permissions are the pairing page's.
- **Problems.** The web view's error codes become the kinds of problem in `GlasshouseErrors`: a name that does not
  resolve is "not found", a connection that fails is "refused", and both are "offline" when the phone has no network.
  With no network the page is not asked for at all: the web view would wait on a connection it had open before the
  network went, so the page says "offline" at once and Reload asks again. A web view whose process stopped is replaced
  and the page says "stopped working" with Reload.
- **Restricted mode.** The page is always in it, from the first moment, and nothing switches it off; a small "Restricted
  mode" sign under the address bar says so. The system's basic way is used: `FLAG_SECURE` on the activity's window
  (`SecureWindow`), which the phone keeps out of screenshots, screen recordings, other displays and the picture of the
  app in the recent apps. The flag is the whole window's, so it is on for as long as the page is shown and off on the
  other pages of the app. Beyond the window:
  - **Touch menu.** `RestrictedWebView` filters the web view's own selection menu to Select all and Paste, so the menu
    never offers Copy, Cut, Share or any other action that takes text out (on the emulator the web view's menu on a
    held word is Copy, Share, Select all and Read aloud, and the person gets Select all). The items are known by their
    titles, the system's words for them, because their ids belong to the web view's package; an item whose title is not
    one of them is taken out. The web view has no menu on a picture, so there is no Save image. Dragging selected text
    out of the app is not blocked.
  - **Clipboard.** A page can write to the clipboard with its own commands and the phone has no way to refuse it. While
    the web view has the focus and so does the app's window, which they have when a page's command runs, a change of the
    clipboard is the page's; when the app goes to the background (`onStop`) `GlasshouseClipboard` takes it off, and puts
    back what was there before, or clears it if nothing was, or if the phone refuses the old clip (one from another app
    can hold an address this app may not hand on). A copy made anywhere else (the address bar, another app) is the
    person's and is kept. Android 10 and later let an app read the clipboard only while its window has the focus, and
    tell it nothing of a change it cannot read, so the clipboard is read when the window gets the focus
    (`onWindowFocusChanged`), and a clip that is not the one the page put there is the person's. Android 12 and later tell
    the person that the app pasted from the clipboard when it reads a clip another app made. Clipboard managers that
    keep a history are not worked around.
  - **Download, print, share.** A download (a link with `download`, or a file the server sends to be saved) is refused
    by the web view's download listener, which starts none. A document-start script, run in every frame before the
    page's own, makes `window.print` do nothing and `navigator.share` reject as a refusal, and tells the app, which
    says so in one line under the sign for six seconds. The script needs `androidx.webkit`'s document-start script and
    web message listener; a web view without them does not open the page.

The page tests (`GlasshouseTest`, `GlasshouseRestrictedTest`, `GlasshouseRestartTest`) open real pages from a test server on the computer over
`https`. The debug build, and only the debug build, trusts that server's certificate: `./gradlew :androidApp:assembleDebug`
makes two certificates with `keytool` (`generateGlasshouseTestCertificates`), puts the public part of the first into the
debug resources, and `src/debug/res/xml/network_security_config.xml` trusts it through `debug-overrides`, which an app
that is not debuggable ignores. The second certificate is trusted by nothing and stands for a site that cannot be trusted.
`scripts/glasshouse_test_server.py` serves the made-up pages with them and logs what it was asked for; the tests read
that log through the same connection. To run everything on an emulator that is yours alone (the tests turn its network
off and on), and to take a screenshot of each state in both themes:

```sh
./gradlew :androidApp:assembleDebug :androidApp:assembleDebugAndroidTest
python3 scripts/test-glasshouse.py --device emulator-5554 --screenshots <folder>
```

## The Dashboard page

`feature/dashboard` shows the computer's dashboard from the copy on the phone, so it works with the computer off and never
asks it for anything. The phone counts nothing: the computer sends the numbers of each widget of its document for each
day that has activity and for each week (Monday to Sunday) that has, in the computer's time zone (`core/sync`,
`SyncDashboard.kt`; `agent/src/phone/sync.rs` on the computer's side), and the copy keeps each answer under the exact
input it was counted for (the widget and the days with their label and their start and end in UTC). So `DashboardHolder`
names the days the way the computer does (the zone the copy remembers, a day from its midnight to the next one) and asks
the copy for those. What follows from that:

- The page counts a day or a week, and it moves by one of them. A widget draws the numbers for the page's day or week
  whatever period the document names for it: the copy holds no month, no "last 7 days" and no "last 30 days". The
  summary of a day always counts one day, the day the page stands on.
- A day or a week the copy has no entry for says that the computer has not sent it, and shows no zeros. The computer
  sends only days with activity, so a day with none and a day not sent look the same to the phone.
- "Today" is today in the computer's zone, and the page does not go past it: nothing is counted for the future.
- The page shows when the copy was last brought up to date (`SyncRecord.syncedAt`), the projects hidden on the computer
  (`hiddenProjectCount`), and an empty state when the copy holds no document: before the first copy, after Unpair and
  after everything about the person is erased on the computer.
- The numbers hold no text of a message, and the page keeps it so.

`DashboardHolderTest` shows each of these with a fake copy, and `DashboardFormatTests` how the iPhone words time and days.
The iPhone page is tested against the computer's test program in the shared Simulator
(`scripts/test-phone-pairing.py --dashboard`, like `--sync`; the script asks the program for sessions on four days
counted from today and tells the test what to look for).

## Rules

Data flows one way: the store, the state holder, the screen. A screen shows a state and passes on
the user's actions; it loads nothing. Three rules are checked by `tools/check_structure.py`, which
reads Kotlin, Gradle and Swift files and prints `file:line: message` for each violation. They fail
`./gradlew check` and `iosApp/check.sh`, which `.github/workflows/mobile.yml` runs.

1. A feature uses no other feature. In Gradle: no `project(":feature:x")` or `projects.feature.x` of
   another feature. In Kotlin: no mention of another feature's package. In Swift: a file under
   `iosApp/Features/<Name>` names no public top-level type that another feature declares, in Swift
   (`struct`, `class`, `enum`, `protocol`, `actor`, `typealias`, with any modifiers and attributes
   in any order but not `private` or `fileprivate`) or in Kotlin (`class`, `interface`, `object`,
   `typealias`, with any modifiers but not `private`, `internal` or `protected`). Swift sees all
   Kotlin through `ConstructShared`, so it is matched by name. A type the file declares itself,
   nested or private, is its own in that file, and a nested Kotlin type is its feature's own: Swift
   shows it qualified (`Outer.MemoryState`) and the check cannot tell a qualified use from a bare
   one, so it lets the bare name pass. Name a feature's public types after the feature
   (`MemoryState`, not `State`),
   because a bare name that another feature declares also fails in a feature that uses the
   SwiftUI or core name. A feature may use `core/*`. A feature folder name maps between Kotlin and
   Swift without `_`, `-` and case: `shopping_cart` is `ShoppingCart`.
1. A screen names no store, state holder or `Construct` type. A screen is any file with `Screen` in
   its name: in a feature's `src/*Main` source sets for Kotlin, under `iosApp/Features/<Name>` for
   Swift. Each feature folder that exists on a side (a `feature/<name>` module, an
   `iosApp/Features/<Name>` folder) needs at least one there, so a renamed screen or a feature with
   no screen is an error (`required: …`), not a way out. Names ending in `Construct`, `Holder`, `Api`
   (but not `Experimental…Api`), `Repository`, `Store`, `Dao`, `Database` or `Model`, and `UiScope`,
   are refused, and a Kotlin screen imports no `core.construct` or `core.database` package. The
   holder is built outside the screen (on the iPhone in `<Name>Page.swift`) and the screen gets its
   state.
1. A screen has no colour or size literal: no `16.dp`, `14.sp`, `Color(...)`, `0xFF...`,
   `Color.Red`, `Color(red:...)`, `.red`, `.padding(8)`, `.frame(width: 320)`, `CGFloat(24)`. Colours
   and spacing come from `core/designsystem`; text sizes, radii and control sizes come from the
   app's own theme.

A fourth rule, that spacing, colours, text sizes and radii are set once in the theme of each app,
is reviewed, not checked.

What the checks do not catch: they are regular expressions over text, not a parser. All of these
pass: a literal that does not look like the patterns above (a size computed from a number, a colour
from a hex string at run time, `0xFF000000L`, `(8).dp`); a store type with a name outside the list;
a feature that reaches another feature through a top-level function or a type declared in the middle
of a line; and code inside a string interpolation (`"${Color.Red}"`, `"\(CGFloat(4))"`), which is
not scanned. A string whose interpolation holds an escaped quote can also hide the rest of that one
line. The check fails when it finds no feature or no `iosApp/Features` folder, so a moved folder is
an error, not a pass.

`./gradlew check` runs the script on the real tree (`checkStructure`) and its tests
(`checkStructureTests`). The tests show each pattern and each name a pattern lists failing on a small
fixture tree built in a temporary folder (no real file is touched), what the comment and string
stripping hides and does not hide, the command's exit status and output, and that a copy of
`feature/template` passes with no other file changed.

## Add a feature

1. Copy `feature/template` to `feature/<name>` and rename what has `template` or `Template` in its
   name or text: the folder, the Kotlin package, the namespace in `build.gradle.kts`, and the
   `TemplateState`, `TemplateHolder` and `TemplateScreen` files and classes. Nothing else needs a
   change for the checks or for the iPhone framework: `settings.gradle.kts` includes every folder
   under `feature/` that has a build file, and `shared/build.gradle.kts` links every feature but
   the template, which is only there to copy.
1. Fill the state, the holder (what it reads from `Construct`) and the Android screen. Keep the test
   with `FakeConstruct`.
1. For the iPhone, copy `iosApp/Features/Memory` to `iosApp/Features/<Name>` and rename it the same
   way: `<Name>Screen.swift` draws a state, `<Name>Page.swift` builds the holder. The Xcode project
   picks the files up by folder.
1. Show it in the app: add the dependency on `:feature:<name>` in `androidApp/build.gradle.kts` and
   put the screen in the navigation of `MainActivity.kt` and `iosApp/App/RootView.swift`.
1. Run `./gradlew check` (it runs the structure checks) and `./iosApp/check.sh` (the same checks, then
   SwiftLint, for a change that touches only Swift).

## What the Mac needs

| Need                                                                                                                                                                                | What it costs                                                                                                |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| An Apple Silicon Mac                                                                                                                                                                |                                                                                                              |
| Xcode (the app, not only the Command Line Tools) and an iOS simulator runtime                                                                                                       | Xcode 27.2 with the iOS 27.2 runtime; several GB                                                             |
| JDK 17, with `JAVA_HOME` set to it                                                                                                                                                  | the version the build was proven with                                                                        |
| The Android SDK (`ANDROID_HOME`, or `sdk.dir` in an uncommitted `local.properties`) with platform 37.2, build tools and the emulator image `android-37.0` `google_apis` `arm64-v8a` | several GB                                                                                                   |
| `xcodegen` and `swiftlint`: `brew install xcodegen swiftlint`                                                                                                                       | 2.48.0 and 0.65.1 were used                                                                                  |
| Python 3.9 or newer, for the structure checks                                                                                                                                       |                                                                                                              |
| Disk for the caches                                                                                                                                                                 | the Gradle caches about 2 GB; the Kotlin/Native toolchain about 1.6 GB, downloaded by the first iPhone build |
| A network for the first build                                                                                                                                                       | public packages only; no account and no token                                                                |

Every version of a library is in `gradle/libs.versions.toml`.

## Build and check

```sh
cd mobile
export JAVA_HOME=$(/usr/libexec/java_home -v 17)
./gradlew check                                  # tests, ktlint, detekt, the structure checks
./gradlew ktlintFormat                           # fix ktlint findings
python3 -I tools/check_structure.py              # only the structure rules, all of them
./iosApp/check.sh                                # the iPhone check: the structure rules, then SwiftLint
./gradlew :androidApp:assembleDebug              # build the Android app without installing it
./gradlew :shared:linkDebugFrameworkIosSimulatorArm64  # link the Kotlin framework for the Simulator
```

`.github/workflows/mobile.yml` runs these commands, and the Simulator build of "Run the iPhone app", for every
pull request that changes `mobile/` and for every merge to `main`.

## Run the Android app

Start an emulator (create one first with `avdmanager` or Android Studio from the image above), then
install and open the app. Debug builds fill an empty copy with the sample data.

```sh
emulator -avd <name> -no-audio &                 # add -no-window to run it hidden
adb wait-for-device shell 'while [ "$(getprop sys.boot_completed)" != 1 ]; do sleep 1; done'
cd mobile
./gradlew :androidApp:installDebug
adb shell am start -n cf.construct.mobile/cf.construct.mobile.android.MainActivity
```

## Connect to a computer

`core/connection` accepts the IPv4 or bracketed IPv6 address shown by the computer. Pairing uses
an eight-digit typed code or a `construct-pair://` QR payload, over the native bridge's TLS 1.3
client. A QR offer pins the certificate before sending its proof; a typed offer asks for the OPAQUE
protocol during the TLS negotiation, and the phone saves the pin and the token only after the
exchange has authenticated the channel. The pairing record stays in Android's Keystore-backed
storage.

`feature/status` watches the connection while the app is in the foreground. A paired phone pings on
the authenticated channel. A revocation received on that channel clears the pairing; a lost route or
a computer that does not answer keeps the pairing and shows the last contact.

On both phones, Settings opens the Computer page: scan the QR code with the camera, or type the
address and the code. Android's scanner uses CameraX and the ZXing core QR reader, the iPhone's uses
`AVFoundation`; both decode frames on the phone without an account or a network call. Each asks for
camera access when the scanner opens. Android asks for local network access on a phone that has that
permission. The iPhone cannot ask for it, so the app reads a refusal from the failed connection and
sends the person to Settings. Unpair removes the pairing and the computer's copied chats and facts,
and keeps the chats started on the phone.

Run the page on a disposable emulator against the test program of the computer's endpoint. The test
slows the app's checks of the computer to two minutes, so that only the Try again button can bring a
lost link back; a debug build has that setting and a release build does not:

```sh
cargo build --locked -p construct-agent --example phone_pairing
./gradlew :androidApp:assembleDebug :androidApp:assembleDebugAndroidTest
# install both APKs, then:
python3 scripts/test-phone-pairing.py --device <serial> \
  --agent <path to the phone_pairing example> --screenshots <folder>
python3 -I scripts/test_phone_pairing_checks.py   # the script's check of the device log
```

The iPhone page runs the same way in the shared test Simulator (see [The test Simulator](#the-test-simulator)). Build
the app and its tests once, then give the script the folder of that build:

```sh
cargo build --locked -p construct-agent --example phone_pairing
cd iosApp && xcodegen generate && cd ..
xcodebuild -project iosApp/Construct.xcodeproj -scheme Construct -derivedDataPath <build folder> \
  -destination 'generic/platform=iOS Simulator' CODE_SIGNING_ALLOWED=YES CODE_SIGN_IDENTITY=- \
  build-for-testing
python3 scripts/test-phone-pairing.py --agent <path to the phone_pairing example> \
  --ios-build <build folder> --ios-device-type <simctl device type> --ios-runtime <simctl runtime> \
  --ios-theme light --screenshots <folder>
```

The build is signed ad hoc for this run (`CODE_SIGN_IDENTITY=-`, no account needed): a Simulator app
without an application identifier is refused every Keychain call by the Simulator of iOS 27, and the
page would show a storage error. The script stops with a message when the build is not signed.
`--ios-theme dark` runs the same test in the dark appearance. `--sync` runs the iPhone sync test
(`UITests/SyncTests`) the same way: the script starts the program with a file of made-up facts and chats, adds a chat
to the file and stops and starts the program when the test asks, and the test follows the first copy (its steps and
counts), the notices, the checks that say nothing, a new chat in the open list, "Sync now" and a computer that does not
answer. The app runs with `--sync-test`, a debug Simulator hook like `--computer-page-test`: it checks every 6 seconds
(`CONSTRUCT_SYNC_TEST_INTERVAL_MS` sets the time in milliseconds, for a step that must not meet a check in the middle
of what it looks at) and waits before each sync and after each report of progress, so a test can see "Syncing" and the
counts rise. A release build has neither. The test asks the script for each step
over a port on the loopback address, so no pairing secret is passed as a launch argument.

## The test Simulator

The scripts that test the iPhone app (`test-phone-pairing.py` with `--ios-build`, `test-glasshouse-ios.py` and
`check-key-not-leaked.sh ios`) all use one Simulator named `Construct Test`, because every booted Simulator takes the
memory of a phone. `scripts/shared_simulator.py` makes it the first time (from the device type and the runtime the
script is given) and never deletes it. At the start of a run it

- waits if another run is using the Simulator, so that two runs never clash,
- boots it and waits until it answers,
- resets its Keychain (so no pairing or key of an earlier run is left), removes the app with its
  files, installs the app again, and sets the light appearance and the normal status bar,

and when the run ends it shuts the Simulator down. A `Construct Test` Simulator of another device type or runtime, or a
second one of the same name, is deleted when a run asks for the Simulator, so do not use that name for your own. The
Android emulator is not part of this: the Android scripts use the emulator you start.

`python3 -B -I scripts/test_shared_simulator_checks.py` checks the helper against a fake `simctl`, with no Simulator.

## Sync the copy from the computer

`core/sync` copies what the computer holds into the phone's copy over the paired connection. The first
copy reads the facts, the chats and the dashboard in three steps and reports how many items each step has
copied, and how many there are once the step knows (the facts and the numbers are counted as their pages come).
After that a check asks the computer for a token of each part and copies only the parts whose token
changed (a chat is read from the item the copy already has). What the sync reads is applied in one
transaction, so a reader of the copy sees it as it was or as it is, never between; what the computer no
longer has is removed from the copy. Emptying the copy (another computer, or an erase on the computer) is a
transaction of its own that commits first, so a refill that is cut leaves the copy empty, as the computer is.
A copy or a store that cannot be read or written cuts the sync and leaves the copy as it was; the next check
tries again.

The copy records the computer it came from by its certificate fingerprint, not its address. Pairing again
with the same computer keeps the copy; pairing with another one removes what came from the first when the
pairing completes, before any page can show it. A computer drops its erase marker when it unpairs its last
phone, so pairing again with the same computer also forgets the marker the copy held, and the next check takes
the computer's marker as the first it has seen. The copy keeps a marker only once a sync has been applied (the
first pairing takes the computer's marker and wipes nothing, for the same reason). So an erase on the computer
while the phone was unpaired, or between a pairing and the first sync that is applied, is not seen as an erase:
the sync still removes what the computer no longer has, but the chats started on the phone stay. The chats
started on the phone are removed only when a paired computer tells the phone, after that first sync, that
everything about the person was erased. Nothing goes to the computer.

`SyncHolder` in `feature/status` is the state the pages show. The iPhone app builds it (`App/SyncModel.swift`) and
shows it in three places. The loading screen of the first copy (`Features/Status/FirstCopyScreen.swift`) lies over
the tabs while a first copy runs and the computer answers, so the page the person was on is still there when it
ends: three steps with their counts, a bar filled a third for each step, no time left. The notice at the top of a
page (`Core/SyncNoticeRow.swift`) is the first row of the Chat, Memory and Settings pages, so it goes away with the
page when they scroll; the frame gives it to every page but the Glasshouse page, which has nothing to sync, and
nothing shows while all is fine. "Sync now" and the time of the last sync are on the Computer page. A pairing that
completes starts the first copy at once. The Android app does not build the holder yet. The holder runs a sync when
the app comes to the front, every minute while it
stays there, and when a person asks for one. It says "syncing" and "just synced" only for a sync the person
waits for, and "not reachable" once when the computer stops answering. A sync that is cut shows no notice and
leaves "last synced" where it was. The other link states (not paired, no network permission, a pairing that
cannot be opened, an app or a computer to update) are the Computer page's to show.

The tests against the fake computer run with `./gradlew check`. `RealAgentSyncTest` runs the same
engine against the test program of the computer's endpoint, over real TLS. It is skipped unless the
program's path is given, and the pull request workflow does not build that program yet, so it runs where
someone builds it:

```sh
cargo build --locked -p construct-agent --example phone_pairing
cd mobile
CONSTRUCT_PHONE_PAIRING_EXAMPLE=<path to the phone_pairing example> ./gradlew :core:sync:testAndroidHostTest
```

## Run the iPhone app

A Simulator build links the debug `iosSimulatorArm64` framework and a device build the release
`iosArm64` framework (`KOTLIN_FRAMEWORK_DIR` in `iosApp/project.yml`). Xcode builds the Kotlin
framework first, through Gradle, so `JAVA_HOME` should point at JDK 17 (the version the build was
proven with) in the shell that runs `xcodebuild`.
The first build downloads the Kotlin/Native toolchain and is slow.

```sh
cd mobile/iosApp
xcodegen generate                                # writes Construct.xcodeproj (ignored by git)
xcodebuild -project Construct.xcodeproj -scheme Construct \
  -destination 'platform=iOS Simulator,name=<a simulator you have>' test
xcrun simctl boot '<a simulator you have>'       # skip if it is booted
xcodebuild -project Construct.xcodeproj -scheme Construct \
  -destination 'platform=iOS Simulator,name=<a simulator you have>' \
  -derivedDataPath build build
xcrun simctl install booted build/Build/Products/Debug-iphonesimulator/Construct.app
xcrun simctl launch booted "$(sed -n 's/^ *APP_BUNDLE_ID: *//p' project.yml)"
```

`xcrun simctl list devices available` shows the simulator names. Open `Simulator.app` to see the
screen, or run `xcrun simctl io booted screenshot shot.png`.

## Android release

```sh
cd mobile
scripts/release-android.sh
```

The script builds the release APK, signs it, and checks the signature with `apksigner`. The key
comes only from the environment (the keystore path must be absolute): `ANDROID_KEYSTORE_PATH`, `ANDROID_KEY_ALIAS`,
`ANDROID_KEYSTORE_PASSWORD` and, for a keystore whose key has its own password,
`ANDROID_KEY_PASSWORD`. Without the key the script stops and leaves no APK. Only the v2 signature
scheme is used.

The application id and the flag that says whether it is the final one are in `gradle.properties`
(`constructApplicationId`, `constructApplicationIdIsFinal`).

- While the id is not final, nothing is published. The script installs the APK on the running
  emulator or device (it first uninstalls the app, because an APK signed with another key cannot be installed over it; that erases its data) and
  saves a light and a dark screenshot in a folder it prints.
- When the id is final, the script publishes the signed APK as a GitHub pre-release. The tag is
  `android-v<constructVersionName>` and points at the pushed commit the APK was built from; the
  script stops when the tree has changes, the commit is not pushed, the release already exists, or the version code is not higher than that of an earlier release.
  Raise `constructVersionName` and `constructVersionCode` in `gradle.properties` for each release.
  With `RELEASE_DRAFT=1` (or `true`, `yes`) the release is a draft.

## iPhone build

```sh
cd mobile
scripts/build-ios.sh
```

The bundle id and the flag that says whether it is the final one are in `iosApp/project.yml`
(`APP_BUNDLE_ID`, `APP_BUNDLE_ID_IS_FINAL`), with the Apple team (`DEVELOPMENT_TEAM`). `JAVA_HOME`
should point at JDK 17; without it the script looks for one, also in Homebrew.
`IOS_SIMULATOR='<name>'` picks the simulator; by default it is the first single-screen iPhone
(`xcrun simctl list devices available` lists them).

- While the id is not final, nothing is sent to Apple and no key is read. The script builds the app
  for an iPhone simulator, installs and launches it, and saves a light and a dark screenshot in a
  folder it prints. It boots the simulator when none is running and shuts it down afterwards.
- When the id is final, the script first makes the same Simulator build and screenshots, for those
  who do not have the TestFlight build. Then it archives the Release build for a device with
  automatic signing and sends it to TestFlight; a failed screenshot stops the run before anything
  is sent. Set the exact TestFlight build number in `CURRENT_PROJECT_VERSION` in
  `iosApp/project.yml` as part of the feature pull request. Use `<YYMMDD>.<count of the day>`
  in UTC (for example, `261122.2`). Before merging, check current `main` and App Store Connect
  and update the pull request if the number would collide or fall behind. Fetch `origin/main`
  and run the upload from a clean checkout of the merged commit. Before uploading, the script checks
  the archive's version and build number against the project and leaves the project file unchanged.
  Keep `MARKETING_VERSION` unless the release calls for a new app version. The
  upload uses the App Store Connect API key when `APP_STORE_CONNECT_KEY_PATH` (the `.p8` file, kept
  outside the repository), `APP_STORE_CONNECT_KEY_ID` and `APP_STORE_CONNECT_ISSUER_ID` are all set,
  and otherwise the Apple account Xcode is signed in with (Xcode, Settings, Accounts). With neither,
  or with only some of the three variables, it stops before building. The key values are passed to
  `xcodebuild` and never printed.

Run `scripts/build-ios.sh --archive-only` to build an unsigned device archive in
`build/ios-archive-check` and verify its version and build number locally. This mode skips the
Simulator and does not sign in to App Store Connect or upload.

App Store Connect processes a build for some minutes. It then shows in TestFlight for the testers of
an internal group; with automatic distribution on that group, every new build reaches them by
itself. A tester installs it from the TestFlight app on the iPhone.

The first upload from a Mac needs a person once:

- Xcode is signed in with the Apple ID of a member of the team. The Developer role is enough: the
  build is signed for distribution by the team's cloud-managed certificate, so no distribution key
  is on the Mac. If that signing is refused, the role lacks "Access to Cloud Managed Distribution
  Certificate" (App Store Connect, Users and Access).
- The archive signs with the person's local "Apple Development" key, and macOS asks for the login
  keychain password. Click "Always Allow": "Allow" covers one signing only, so the next build asks
  again, and a build with nobody at the Mac waits for it. A new development certificate asks once
  more.
