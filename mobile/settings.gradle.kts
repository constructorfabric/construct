pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode = RepositoriesMode.FAIL_ON_PROJECT_REPOS
    repositories {
        google()
        mavenCentral()
        // The Kotlin part of the Rust TLS stack's certificate check on Android, published by its
        // authors as a plain Maven repository. Only its own group comes from here.
        exclusiveContent {
            forRepository {
                maven(
                    "https://github.com/rustls/rustls-platform-verifier/raw/maven-archive/android-release-support/maven/",
                )
            }
            filter { includeGroup("org.rustls") }
        }
    }
}

rootProject.name = "construct-mobile"

include(":core:model")
include(":core:construct")
include(":core:database")
include(":core:chat")
include(":core:keys")
include(":core:designsystem")
// Every folder under feature/ with a build file is a feature, so a copy of feature/template needs
// no line here.
file("feature")
    .listFiles { folder -> File(folder, "build.gradle.kts").isFile }
    .orEmpty()
    .sortedBy { it.name }
    .forEach { include(":feature:${it.name}") }
include(":shared")
include(":androidApp")
