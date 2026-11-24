plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.android.kotlin.multiplatform.library)
}

kotlin {
    android {
        namespace = "cf.construct.mobile.core.connection"
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
        withHostTest {}
    }
    iosArm64()
    iosSimulatorArm64()

    sourceSets {
        commonMain.dependencies {
            implementation(project(":core:chat"))
            api(project(":core:keys"))
            implementation(libs.serialization.json)
            implementation(libs.coroutines.core)
        }
        commonTest.dependencies {
            implementation(libs.kotlin.test)
            implementation(libs.coroutines.test)
        }
    }
}
