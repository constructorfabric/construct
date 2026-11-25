plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.android.kotlin.multiplatform.library)
}

kotlin {
    android {
        namespace = "cf.construct.mobile.core.sync"
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
            api(project(":core:connection"))
            api(project(":core:database"))
            implementation(libs.serialization.json)
            implementation(libs.coroutines.core)
        }
        commonTest.dependencies {
            implementation(libs.kotlin.test)
            implementation(libs.coroutines.test)
        }
        getByName("androidHostTest").dependencies {
            implementation(libs.sqldelight.sqlite.driver)
            // The test that runs the agent's own test program speaks to it with a transport written for the JVM.
            implementation(project(":core:chat"))
        }
    }
}
