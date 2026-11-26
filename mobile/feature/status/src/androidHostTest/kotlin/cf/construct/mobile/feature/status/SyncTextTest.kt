package cf.construct.mobile.feature.status

import java.time.ZoneId
import java.util.Locale
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class SyncTextTest {
    private val paris = ZoneId.of("Europe/Paris")

    @Test
    fun aCopyThatWasNeverSyncedHasNoTime() {
        assertNull(formatSyncTime(null, paris, Locale.UK))
    }

    @Test
    fun theTimeIsShownInThePhonesZone() {
        assertEquals("18 Nov 2026, 15:32", formatSyncTime("2026-11-18T14:32:00Z", paris, Locale.UK))
        assertEquals("18 Nov 2026, 14:32", formatSyncTime("2026-11-18T14:32:00Z", ZoneId.of("UTC"), Locale.UK))
    }

    @Test
    fun aTimeWithFractionsOfASecondIsRead() {
        assertEquals("18 Nov 2026, 15:32", formatSyncTime("2026-11-18T14:32:00.123456Z", paris, Locale.UK))
    }

    @Test
    fun aTimeWithAnOffsetIsRead() {
        assertEquals("18 Nov 2026, 15:32", formatSyncTime("2026-11-18T16:32:00+02:00", paris, Locale.UK))
    }

    @Test
    fun theWordsFollowThePhonesLanguage() {
        // Java 20 and later put a narrow no-break space before "PM", and Java 17 a plain one: both read the same.
        val american = formatSyncTime("2026-11-18T14:32:00Z", paris, Locale.US)?.replace('\u202f', ' ')
        assertEquals("Nov 18, 2026, 3:32 PM", american)
    }

    @Test
    fun aDamagedTimeIsTheSameAsNone() {
        assertNull(formatSyncTime("yesterday", paris, Locale.UK))
        assertNull(formatSyncTime("", paris, Locale.UK))
    }
}
