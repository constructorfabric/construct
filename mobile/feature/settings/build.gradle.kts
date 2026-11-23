plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.android.kotlin.multiplatform.library)
    alias(libs.plugins.kotlin.compose)
}

kotlin {
    android {
        namespace = "cf.construct.mobile.feature.settings"
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
            api(project(":core:keys"))
            api(project(":core:chat"))
            api(libs.coroutines.core)
            implementation(libs.compose.runtime)
        }
        androidMain.dependencies {
            implementation(project(":core:designsystem"))
            implementation(project.dependencies.platform(libs.compose.bom))
            implementation(libs.compose.material3)
            implementation(libs.compose.icons.core)
        }
        commonTest.dependencies {
            implementation(libs.kotlin.test)
            implementation(libs.coroutines.test)
        }
    }
}
