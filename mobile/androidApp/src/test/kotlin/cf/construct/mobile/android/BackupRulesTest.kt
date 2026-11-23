package cf.construct.mobile.android

import cf.construct.mobile.core.keys.MODEL_KEY_FILE
import cf.construct.mobile.core.keys.MODEL_SETTINGS_PREFERENCES
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The key stays out of every backup: the app takes none, and the rules keep it out should that change. */
class BackupRulesTest {
    private fun resource(path: String) = File("src/main/$path").readText()

    private val manifest = resource("AndroidManifest.xml")

    @Test
    fun theAppTakesNoBackup() {
        assertTrue(manifest.contains("""android:allowBackup="false""""))
    }

    @Test
    fun theManifestPointsAtBothRuleFiles() {
        assertTrue(manifest.contains("""android:dataExtractionRules="@xml/data_extraction_rules""""))
        assertTrue(manifest.contains("""android:fullBackupContent="@xml/backup_rules""""))
    }

    @Test
    fun everyRuleFileExcludesTheKeyFileAndThePreferences() {
        for (rules in listOf("backup_rules", "data_extraction_rules")) {
            val text = resource("res/xml/$rules.xml")
            assertTrue(text.contains("""path="$MODEL_KEY_FILE""""), "$rules leaves the key file in")
            assertTrue(text.contains("""path="$MODEL_SETTINGS_PREFERENCES.xml""""), "$rules leaves the preferences in")
        }
    }

    @Test
    fun theTwoDataExtractionPathsBothExcludeTheKey() {
        val text = resource("res/xml/data_extraction_rules.xml")

        assertEquals(2, Regex("""<exclude domain="file" path="$MODEL_KEY_FILE" />""").findAll(text).count())
    }
}
