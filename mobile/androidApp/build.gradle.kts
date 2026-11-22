import com.android.build.api.variant.HostTestBuilder

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
        versionName = providers.gradleProperty("constructVersionName").get()
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

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
}

dependencies {
    implementation(project(":core:chat"))
    implementation(project(":core:construct"))
    implementation(project(":core:database"))
    implementation(project(":core:designsystem"))
    implementation(project(":feature:memory"))
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.material3)
    implementation(libs.compose.activity)
    testImplementation(libs.kotlin.test.junit)
    androidTestImplementation(libs.kotlin.test.junit)
    androidTestImplementation(libs.androidx.test.runner)
    testImplementation(libs.sqldelight.sqlite.driver)
}

// The Rust library is built before the app packs it, and the fixtures of the chat core's tests are
// written before the tests compile.
tasks.configureEach {
    if (name.startsWith("merge") && name.endsWith("JniLibFolders")) dependsOn(":buildChatCoreAndroid")
    if (name.contains("AndroidTest") && name.startsWith("compile")) dependsOn(":core:chat:generateFixtures")
}
