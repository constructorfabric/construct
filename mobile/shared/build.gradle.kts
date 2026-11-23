plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.skie)
}

// Every feature module but the template, which is only there to copy, so a copy of it needs no line
// here.
val featurePaths =
    rootProject.subprojects
        .map { it.path }
        .filter { it.startsWith(":feature:") && it != ":feature:template" }
        .sorted()

// The one Kotlin framework the iPhone app links. It re-exports every module the Swift code uses, so
// the app has a single framework and a single copy of the Kotlin runtime.
kotlin {
    listOf(iosArm64(), iosSimulatorArm64()).forEach {
        it.binaries.framework {
            baseName = "ConstructShared"
            isStatic = true
            binaryOption("bundleId", "cf.construct.mobile.shared")
            export(project(":core:model"))
            export(project(":core:chat"))
            export(project(":core:construct"))
            export(project(":core:database"))
            export(project(":core:designsystem"))
            featurePaths.forEach { export(project(it)) }
        }
    }

    sourceSets {
        iosMain.dependencies {
            api(project(":core:model"))
            api(project(":core:chat"))
            api(project(":core:construct"))
            api(project(":core:database"))
            api(project(":core:designsystem"))
            featurePaths.forEach { api(project(it)) }
        }
    }
}

skie {
    analytics {
        disableUpload.set(true)
    }
}
