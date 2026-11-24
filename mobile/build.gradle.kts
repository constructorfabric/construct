import io.gitlab.arturbosch.detekt.Detekt
import org.jlleitschuh.gradle.ktlint.KtlintExtension

plugins {
    base
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.android.kotlin.multiplatform.library) apply false
    alias(libs.plugins.kotlin.multiplatform) apply false
    alias(libs.plugins.kotlin.serialization) apply false
    alias(libs.plugins.kotlin.compose) apply false
    alias(libs.plugins.sqldelight) apply false
    alias(libs.plugins.skie) apply false
    alias(libs.plugins.detekt) apply false
    alias(libs.plugins.ktlint) apply false
}

val ktlintVersion = libs.versions.ktlint.get()

allprojects {
    apply(plugin = "io.gitlab.arturbosch.detekt")
    apply(plugin = "org.jlleitschuh.gradle.ktlint")

    extensions.configure<KtlintExtension> {
        version.set(ktlintVersion)
    }

    tasks.withType<Detekt>().configureEach {
        buildUponDefaultConfig = true
        config.setFrom(rootProject.file("config/detekt/detekt.yml"))
        // Every source set of the module, whatever its name.
        setSource(fileTree("src") { include("**/*.kt") })
        exclude("**/build/**")
    }
}

// The structure rules read Kotlin, Gradle and Swift files, so one small script checks them all; its
// own tests break each rule on a fixture. `check` runs both.
val structureChecks =
    listOf("checkStructure" to "check_structure.py", "checkStructureTests" to "test_check_structure.py")
        .map { (taskName, script) ->
            tasks.register<Exec>(taskName) {
                group = "verification"
                description = "Runs tools/$script."
                commandLine("python3", "-I", "tools/$script")
            }
        }

tasks.named("check") { dependsOn(structureChecks) }

// The chat core is Rust (the `chat-bridge` crate). scripts/build-core.sh builds it into one folder
// outside the repository; these tasks run it only when a Rust source, the lock file or the script
// changed, so a Gradle or Xcode build with unchanged sources does not start cargo.
val chatCoreBuildDir: Provider<File> =
    providers
        .environmentVariable("CONSTRUCT_CORE_BUILD_DIR")
        .orElse(providers.systemProperty("user.home").map { "$it/Library/Caches/construct-chat-core" })
        .map { file(it) }
extra["coreBuildDir"] = chatCoreBuildDir

val rustSources =
    layout.projectDirectory.dir("..").asFileTree.matching {
        // The crates the library is built from; scripts/build-core.sh stops when a new one appears.
        include("Cargo.toml", "Cargo.lock", "rust-toolchain.toml")
        include("tools/phone.json")
        include("chat-bridge/**", "chat-core/**", "connectors/**")
        exclude("**/tests/**", "**/target/**")
    }

fun buildChatCore(
    name: String,
    what: String,
    libraries: List<String>,
) = tasks.register<Exec>(name) {
    group = "build"
    description = "Builds the chat core for $what with scripts/build-core.sh."
    inputs.files(rustSources)
    inputs.file("scripts/build-core.sh")
    // The platform versions the script reads.
    inputs.file("gradle/libs.versions.toml")
    inputs.file("iosApp/project.yml")
    inputs.property("buildDir", chatCoreBuildDir.map { it.path })
    outputs.files(chatCoreBuildDir.map { dir -> libraries.map { File(dir, it) } })
    commandLine("scripts/build-core.sh", what)
    environment("CONSTRUCT_CORE_BUILD_DIR", chatCoreBuildDir.get().path)
}

buildChatCore(
    "buildChatCoreIos",
    "ios",
    listOf("ios/device/libconstruct_chat_bridge.a", "ios/simulator/libconstruct_chat_bridge.a"),
)
buildChatCore(
    "buildChatCoreAndroid",
    "android",
    listOf("android/arm64-v8a/libconstruct_chat_bridge.so", "android/x86_64/libconstruct_chat_bridge.so"),
)
