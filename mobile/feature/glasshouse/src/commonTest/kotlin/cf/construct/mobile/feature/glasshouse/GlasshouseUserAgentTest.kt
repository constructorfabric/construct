package cf.construct.mobile.feature.glasshouse

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

class GlasshouseUserAgentTest {
    private val android =
        "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) " +
            "Version/4.0 Chrome/126.0.0.0 Mobile Safari/537.36"
    private val iphone =
        "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) " +
            "Mobile/15E148"

    @Test
    fun theTokenIsPutLastAndTheRestStays() {
        assertEquals("$android Construct/0.1.1", GlasshouseUserAgent.of(android, "0.1.1"))
        assertEquals("$iphone Construct/0.1.1", GlasshouseUserAgent.of(iphone, "0.1.1"))
    }

    @Test
    fun theTokenOfTheAppIsNotRepeated() {
        val once = GlasshouseUserAgent.of(android, "0.1.1")

        assertEquals(once, GlasshouseUserAgent.of(once, "0.1.1"))
        assertEquals(1, Regex("Construct/").findAll(GlasshouseUserAgent.of(once, "0.1.1")).count())
    }

    @Test
    fun aTokenOfAnotherVersionOfTheAppIsReplaced() {
        val older = GlasshouseUserAgent.of(android, "0.1.0")

        assertEquals("$android Construct/0.1.1", GlasshouseUserAgent.of(older, "0.1.1"))
    }

    @Test
    fun anyTokenOfTheAppsOwnVersionIsTakenOutFromTheMiddle() {
        val base = "Mozilla/5.0 shell/0.1.1 AppleWebKit/537.36 Chrome/126.0.0.0"

        assertEquals(
            "Mozilla/5.0 AppleWebKit/537.36 Chrome/126.0.0.0 Construct/0.1.1",
            GlasshouseUserAgent.of(base, "0.1.1"),
        )
    }

    @Test
    fun theChromeTokenStaysBecauseSitesReadIt() {
        assertTrue(GlasshouseUserAgent.of(android, "0.1.1").contains(" Chrome/126.0.0.0 "))
    }

    @Test
    fun anEmptyBaseGivesTheTokenAlone() {
        assertEquals("Construct/0.1.1", GlasshouseUserAgent.of("", "0.1.1"))
        assertEquals("Construct/0.1.1", GlasshouseUserAgent.of("  ", "0.1.1"))
    }

    @Test
    fun aVersionThatIsNotOneWordIsRefused() {
        assertFailsWith<IllegalArgumentException> { GlasshouseUserAgent.of(android, "") }
        assertFailsWith<IllegalArgumentException> { GlasshouseUserAgent.of(android, "0.1 beta") }
    }
}
