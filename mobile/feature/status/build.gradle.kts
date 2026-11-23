import org.jetbrains.kotlin.gradle.plugin.KotlinPlatformType

plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.android.kotlin.multiplatform.library)
    alias(libs.plugins.kotlin.compose)
}

composeCompiler {
    targetKotlinPlatforms.set(setOf(KotlinPlatformType.androidJvm))
}

kotlin {
    android {
        namespace = "cf.construct.mobile.feature.status"
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
        androidResources { enable = true }
    }
    iosArm64()
    iosSimulatorArm64()

    sourceSets {
        commonMain.dependencies {
            api(project(":core:connection"))
            api(libs.coroutines.core)
        }
        androidMain.dependencies {
            implementation(project(":core:designsystem"))
            implementation(project.dependencies.platform(libs.compose.bom))
            implementation(libs.compose.runtime)
            implementation(libs.compose.material3)
        }
    }
}
