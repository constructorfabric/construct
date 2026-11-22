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
    }
}

rootProject.name = "construct-mobile"

include(":core:model")
include(":core:construct")
include(":core:database")
include(":core:chat")
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
