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
            output.get().asFile.apply {
                parentFile.mkdirs()
                writeText(
                    "package cf.construct.mobile.core.chat\n\n" +
                        "object GeneratedFixtures {\n    val files: Map<String, String> =\n        mapOf(\n$entries,\n        )\n}\n",
                )
            }
        }
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
            tasks.named(interopProcessingTaskName) { dependsOn(":buildChatCoreIos") }
        }
    }

    sourceSets {
        commonMain.dependencies {
            api(libs.serialization.json)
        }
        androidMain.dependencies {
            implementation("net.java.dev.jna:jna:${libs.versions.jna.get()}@aar")
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
