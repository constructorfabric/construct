import io.gitlab.arturbosch.detekt.Detekt
import org.jlleitschuh.gradle.ktlint.KtlintExtension

plugins {
    base
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.android.kotlin.multiplatform.library) apply false
    alias(libs.plugins.kotlin.multiplatform) apply false
    alias(libs.plugins.kotlin.serialization) apply false
    alias(libs.plugins.kotlin.compose) apply false
    alias(libs.plugins.sqldelight) apply false
    alias(libs.plugins.skie) apply false
    alias(libs.plugins.detekt) apply false
    alias(libs.plugins.ktlint) apply false
}

val ktlintVersion = libs.versions.ktlint.get()

allprojects {
    apply(plugin = "io.gitlab.arturbosch.detekt")
    apply(plugin = "org.jlleitschuh.gradle.ktlint")

    extensions.configure<KtlintExtension> {
        version.set(ktlintVersion)
    }

    tasks.withType<Detekt>().configureEach {
        buildUponDefaultConfig = true
        config.setFrom(rootProject.file("config/detekt/detekt.yml"))
        // Every source set of the module, whatever its name.
        setSource(fileTree("src") { include("**/*.kt") })
        exclude("**/build/**")
    }
}

// The structure rules read Kotlin, Gradle and Swift files, so one small script checks them all; its
// own tests break each rule on a fixture. `check` runs both.
val structureChecks =
    listOf("checkStructure" to "check_structure.py", "checkStructureTests" to "test_check_structure.py")
        .map { (taskName, script) ->
            tasks.register<Exec>(taskName) {
                group = "verification"
                description = "Runs tools/$script."
                commandLine("python3", "-I", "tools/$script")
            }
        }

tasks.named("check") { dependsOn(structureChecks) }
