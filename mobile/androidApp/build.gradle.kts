import com.android.build.api.variant.HostTestBuilder
import org.gradle.process.ExecOperations
import java.security.KeyStore
import java.util.Base64
import javax.inject.Inject

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "cf.construct.mobile.android"
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

    defaultConfig {
        // The id and the flag that says it is final live in gradle.properties, which the release script reads too.
        applicationId = providers.gradleProperty("constructApplicationId").get()
        minSdk =
            libs.versions.androidMinSdk
                .get()
                .toInt()
        targetSdk =
            libs.versions.androidCompileSdk
                .get()
                .toInt()
        versionCode = providers.gradleProperty("constructVersionCode").get().toInt()
        versionName =
            providers.gradleProperty("constructVersionName").get().also {
                // The Glasshouse page sends the name as a User-Agent token, which is one word.
                require(it.isNotEmpty() && it.none(Char::isWhitespace)) { "constructVersionName must be one word" }
            }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        // The chat core is built for these two only; this keeps the 32-bit libraries of the JNA AAR out, so
        // a device of another ABI cannot install an app whose core would not load.
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
        // The address the Glasshouse page opens first. Empty means none: the page says so and the person types
        // one. A build sets it with -PconstructGlasshouseStartAddress=<https address>; it is not a setting.
        val glasshouseStart = providers.gradleProperty("constructGlasshouseStartAddress").orElse("").get()
        buildConfigField(
            "String",
            "GLASSHOUSE_START_ADDRESS",
            "\"" + glasshouseStart.replace("\\", "\\\\").replace("\"", "\\\"") + "\"",
        )
    }

    buildFeatures { buildConfig = true }

    sourceSets {
        // The Rust library of each ABI, built outside the repository by scripts/build-core.sh.
        getByName("main").jniLibs.directories.add(
            rootProject.extra
                .get("coreBuildDir")
                .let { (it as Provider<*>).get() }
                .toString() + "/android",
        )
        // The chat core's tests run on the emulator from here, because this module packs the library.
        getByName("androidTest").kotlin.directories.add("../core/chat/src/commonTest/kotlin")
        getByName("androidTest").kotlin.directories.add("../core/chat/build/generated/fixtures")
        getByName("androidTest").assets.directories.add("build/generated/testAssets")
    }

    testOptions {
        // The debug seeding logs a failure; the Android stub of the log must not throw in unit tests.
        unitTests.isReturnDefaultValues = true
    }

    // The release key comes only from the environment. Without the keystore path the release APK is built
    // unsigned; scripts/release-android.sh checks for the key first and refuses to build without it.
    val keystorePath = providers.environmentVariable("ANDROID_KEYSTORE_PATH")
    val keystoreSecret = providers.environmentVariable("ANDROID_KEYSTORE_PASSWORD")
    val keyAliasName = providers.environmentVariable("ANDROID_KEY_ALIAS")
    signingConfigs {
        if (keystorePath.isPresent && keystoreSecret.isPresent && keyAliasName.isPresent) {
            create("release") {
                storeFile = file(keystorePath.get())
                storePassword = keystoreSecret.get()
                keyAlias = keyAliasName.get()
                // A PKCS12 keystore has one password for the store and the key.
                keyPassword = providers.environmentVariable("ANDROID_KEY_PASSWORD").orElse(keystoreSecret).get()
                enableV1Signing = false
                enableV2Signing = true
                enableV3Signing = false
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("release")
        }
    }
}

androidComponents {
    // The release build must be tested too: its seeding differs from the debug build's.
    beforeVariants(selector().withBuildType("release")) { it.hostTests[HostTestBuilder.UNIT_TEST_TYPE]?.enable = true }
    // The certificate the page tests trust goes into the debug build alone.
    onVariants(selector().withBuildType("debug")) { variant ->
        variant.sources.res?.addGeneratedSourceDirectory(
            glasshouseTestCertificates,
            GlasshouseTestCertificates::resDir,
        )
    }
}

dependencies {
    implementation(project(":core:chat"))
    implementation(project(":core:connection"))
    implementation(project(":core:construct"))
    implementation(project(":core:database"))
    implementation(project(":core:designsystem"))
    implementation(project(":core:keys"))
    implementation(project(":feature:chat"))
    implementation(project(":feature:dashboard"))
    implementation(project(":feature:glasshouse"))
    implementation(project(":feature:memory"))
    implementation(project(":feature:settings"))
    implementation(project(":feature:status"))
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.material3)
    implementation(libs.compose.activity)
    implementation(libs.compose.icons.core)
    implementation(libs.camera.camera2)
    implementation(libs.camera.lifecycle)
    implementation(libs.camera.view)
    implementation(libs.zxing.core)
    testImplementation(libs.kotlin.test.junit)
    androidTestImplementation(libs.kotlin.test.junit)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(platform(libs.compose.bom))
    androidTestImplementation(libs.compose.ui.test)
    debugImplementation(platform(libs.compose.bom))
    debugImplementation(libs.compose.ui.test.manifest)
    testImplementation(libs.sqldelight.sqlite.driver)
}

// The Rust library is built before the app packs it, and the fixtures of the chat core's tests are
// written before the tests compile.
tasks.configureEach {
    if (name.startsWith("merge") && name.endsWith("JniLibFolders")) dependsOn(":buildChatCoreAndroid")
    // The lint of the device tests reads the same generated folder.
    if (name.contains("AndroidTest") && (name.startsWith("compile") || name.contains("lint", ignoreCase = true))) {
        dependsOn(":core:chat:generateFixtures")
    }
}

// A TLS server with a certificate no phone trusts, for the test that an https address is refused by the
// system's check and not by a crash. It is made for each build with the JDK's keytool and not stored.
val testKeystore = layout.buildDirectory.file("generated/testAssets/untrusted-server.p12")
val generateTestKeystore =
    tasks.register<Exec>("generateTestKeystore") {
        outputs.file(testKeystore)
        // No inputs: it is made again for every build, so the 30-day certificate never goes stale.
        outputs.upToDateWhen { false }
        doFirst {
            testKeystore
                .get()
                .asFile
                .apply { parentFile.mkdirs() }
                .delete()
        }
        commandLine(
            File(System.getProperty("java.home"), "bin/keytool").path,
            "-genkeypair",
            "-alias",
            "server",
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            "-validity",
            "30",
            "-dname",
            "CN=127.0.0.1",
            "-ext",
            "san=ip:127.0.0.1",
            "-storetype",
            "PKCS12",
            "-storepass",
            "untrusted",
            "-keystore",
            testKeystore.get().asFile.path,
        )
    }
tasks.configureEach {
    if (name.contains("AndroidTest") && (name.endsWith("Assets") || name.contains("lint", ignoreCase = true))) {
        dependsOn(generateTestKeystore)
    }
}

// The certificates of the page tests' server, made for each build and not stored; the README's "The Glasshouse page
// on Android" says what trusts which. The test server reads them from `server` in the output folder.
abstract class GlasshouseTestCertificates : DefaultTask() {
    @get:OutputDirectory
    abstract val resDir: DirectoryProperty

    @get:OutputDirectory
    abstract val serverDir: DirectoryProperty

    @get:Inject
    abstract val exec: ExecOperations

    @TaskAction
    fun make() {
        val raw =
            resDir
                .get()
                .asFile
                .resolve("raw")
                .apply { deleteRecursively() }
        raw.mkdirs()
        val server = serverDir.get().asFile.apply { deleteRecursively() }
        server.mkdirs()
        for (name in listOf("trusted", "untrusted")) {
            val store = temporaryDir.resolve("$name.p12").apply { delete() }
            exec.exec {
                commandLine(
                    File(System.getProperty("java.home"), "bin/keytool").path,
                    "-genkeypair",
                    "-alias",
                    "server",
                    "-keyalg",
                    "RSA",
                    "-keysize",
                    "2048",
                    "-validity",
                    "90",
                    "-dname",
                    "CN=glasshouse-test-$name",
                    // The emulator reaches the Mac at 10.0.2.2; the other two are for a phone with a reverse port.
                    "-ext",
                    "san=ip:10.0.2.2,ip:127.0.0.1,dns:localhost",
                    "-storetype",
                    "PKCS12",
                    "-storepass",
                    PASSWORD,
                    "-keystore",
                    store.path,
                )
            }
            val keys =
                KeyStore
                    .getInstance(
                        "PKCS12",
                    ).apply { store.inputStream().use { load(it, PASSWORD.toCharArray()) } }
            val certificate = pem("CERTIFICATE", keys.getCertificate("server").encoded)
            server.resolve("$name.pem").writeText(certificate)
            server
                .resolve(
                    "$name-key.pem",
                ).writeText(pem("PRIVATE KEY", keys.getKey("server", PASSWORD.toCharArray()).encoded))
            if (name == "trusted") raw.resolve("glasshouse_test_certificate.pem").writeText(certificate)
        }
    }

    private fun pem(
        label: String,
        der: ByteArray,
    ): String =
        "-----BEGIN $label-----\n" +
            Base64.getMimeEncoder(PEM_LINE, "\n".toByteArray()).encodeToString(der) +
            "\n-----END $label-----\n"

    private companion object {
        const val PASSWORD = "glasshouse-test"
        const val PEM_LINE = 64
    }
}

val glasshouseTestCertificates =
    tasks.register<GlasshouseTestCertificates>("generateGlasshouseTestCertificates") {
        resDir.set(layout.buildDirectory.dir("generated/glasshouseTest/res"))
        serverDir.set(layout.buildDirectory.dir("generated/glasshouseTest/server"))
        // Made again when the files are gone or old, so the 90-day certificates never go stale.
        val trusted = layout.buildDirectory.file("generated/glasshouseTest/server/trusted.pem")
        outputs.upToDateWhen {
            trusted.get().asFile.let {
                it.exists() &&
                    System.currentTimeMillis() - it.lastModified() < 60L * 24 * 3600 * 1000
            }
        }
    }
