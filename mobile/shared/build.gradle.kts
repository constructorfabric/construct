plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.skie)
}

// The one Kotlin framework the iPhone app links. It re-exports every module the Swift code uses, so
// the app has a single framework and a single copy of the Kotlin runtime.
kotlin {
    listOf(iosArm64(), iosSimulatorArm64()).forEach {
        it.binaries.framework {
            baseName = "ConstructShared"
            isStatic = true
            binaryOption("bundleId", "cf.construct.mobile.shared")
            export(project(":core:model"))
            export(project(":core:construct"))
            export(project(":core:database"))
            export(project(":core:designsystem"))
            export(project(":feature:memory"))
        }
    }

    sourceSets {
        iosMain.dependencies {
            api(project(":core:model"))
            api(project(":core:construct"))
            api(project(":core:database"))
            api(project(":core:designsystem"))
            api(project(":feature:memory"))
        }
    }
}

skie {
    analytics {
        disableUpload.set(true)
    }
}
