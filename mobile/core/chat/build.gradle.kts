import org.jetbrains.kotlin.konan.target.KonanTarget

plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.android.kotlin.multiplatform.library)
}

val coreBuildDir: Provider<File> by rootProject.extra
val repoRoot = rootProject.layout.projectDirectory.dir("..")

// The files the tests compare with (the tool files and the stored request bodies) become Kotlin
// strings in the test sources, so a test reads them the same way on the emulator and in the
// Simulator.
val fixtureFiles =
    listOf(
        "tools/phone.json",
        "tools/desktop.json",
        "chat-core/tests/golden/phone.json",
        "chat-core/tests/golden/desktop.json",
        "app/src/en.json",
    )
val generateFixtures =
    tasks.register("generateFixtures") {
        val inputFiles = fixtureFiles.map { repoRoot.file(it).asFile }
        val output = layout.buildDirectory.file("generated/fixtures/GeneratedFixtures.kt")
        inputs.files(inputFiles)
        outputs.file(output)
        doLast {
            fun literal(text: String) =
                buildString {
                    append('"')
                    for (c in text) {
                        when {
                            c == '\\' -> append("\\\\")
                            c == '"' -> append("\\\"")
                            c == '$' -> append("\\$")
                            c == '\n' -> append("\\n")
                            c == '\r' -> append("\\r")
                            c == '\t' -> append("\\t")
                            c.code in 32..126 -> append(c)
                            else -> append("\\u" + c.code.toString(16).padStart(4, '0'))
                        }
                    }
                    append('"')
                }
            val entries =
                fixtureFiles.zip(inputFiles).joinToString(",\n") { (name, file) ->
                    "        ${literal(name)} to ${literal(file.readText())}"
                }
            // The length and the byte sum of each file, measured on the file itself, so a test can tell
            // when a mis-escaped literal is not the file.
            val measures =
                fixtureFiles.zip(inputFiles).joinToString(",\n") { (name, file) ->
                    val bytes = file.readBytes()
                    "        ${literal(name)} to Measure(${bytes.size}, ${bytes.sumOf { it.toInt() and 0xFF }})"
                }
            output.get().asFile.apply {
                parentFile.mkdirs()
                writeText(
                    "package cf.construct.mobile.core.chat\n\n" +
                        "class Measure(\n    val bytes: Int,\n    val sum: Int,\n)\n\n" +
                        "object GeneratedFixtures {\n" +
                        "    val files: Map<String, String> =\n        mapOf(\n$entries,\n        )\n\n" +
                        "    val measures: Map<String, Measure> =\n        mapOf(\n$measures,\n        )\n}\n",
                )
            }
        }
    }

// The Kotlin part of the certificate check must be the version of the Rust crate it belongs to, so it
// is read from the lock file (the crate `rustls-platform-verifier-android` carries it).
val platformVerifierVersion: String =
    repoRoot
        .file("Cargo.lock")
        .asFile
        .readLines()
        .let { lines ->
            val at = lines.indexOfFirst { it.trim() == "name = \"rustls-platform-verifier-android\"" }
            check(at >= 0) { "rustls-platform-verifier-android is not in Cargo.lock" }
            lines
                .drop(at + 1)
                .first { it.startsWith("version = ") }
                .substringAfter('"')
                .substringBefore('"')
        }

kotlin {
    android {
        namespace = "cf.construct.mobile.core.chat"
        compileSdk {
            version =
                release(
                    libs.versions.androidCompileSdk
                        .get()
                        .toInt(),
                ) {
                    minorApiLevel =
                        libs.versions.androidCompileSdkMinor
                            .get()
                            .toInt()
                }
        }
        minSdk =
            libs.versions.androidMinSdk
                .get()
                .toInt()
    }

    listOf(iosArm64(), iosSimulatorArm64()).forEach { target ->
        val folder = if (target.konanTarget == KonanTarget.IOS_ARM64) "device" else "simulator"
        target.compilations.getByName("main").cinterops.create("constructChat") {
            definitionFile.set(file("src/nativeInterop/cinterop/constructChat.def"))
            includeDirs(repoRoot.dir("chat-bridge/include"))
            extraOpts("-libraryPath", File(coreBuildDir.get(), "ios/$folder").path)
            // A change to the Rust library must run the interop again: its content is an input.
            val library = File(coreBuildDir.get(), "ios/$folder/libconstruct_chat_bridge.a")
            tasks.named(interopProcessingTaskName) {
                dependsOn(":buildChatCoreIos")
                inputs.files(library)
            }
        }
    }

    sourceSets {
        commonMain.dependencies {
            api(libs.serialization.json)
        }
        androidMain.dependencies {
            implementation("net.java.dev.jna:jna:${libs.versions.jna.get()}@aar")
            implementation("org.rustls:rustls-platform-verifier:$platformVerifierVersion")
        }
        commonTest {
            kotlin.srcDir(generateFixtures.map { it.outputs.files.singleFile.parentFile })
            dependencies {
                implementation(libs.kotlin.test)
            }
        }
        iosTest.dependencies {
            implementation(libs.coroutines.core)
        }
    }
}
