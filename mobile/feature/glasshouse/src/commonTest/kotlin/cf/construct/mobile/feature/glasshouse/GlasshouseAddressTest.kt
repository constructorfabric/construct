package cf.construct.mobile.feature.glasshouse

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class GlasshouseAddressTest {
    @Test
    fun anAddressWithNoSchemeOpensOverHttps() {
        assertEquals("https://example.com", GlasshouseAddress.typed("example.com"))
        assertEquals("https://example.com/docs/a?q=1#top", GlasshouseAddress.typed("example.com/docs/a?q=1#top"))
        assertEquals("https://example.com", GlasshouseAddress.typed("//example.com"))
    }

    @Test
    fun anAddressWithASchemeKeepsItAndOnlyTheSchemeIsLowerCased() {
        assertEquals("https://example.com", GlasshouseAddress.typed("https://example.com"))
        assertEquals("http://example.com", GlasshouseAddress.typed("http://example.com"))
        assertEquals("https://Example.COM/A", GlasshouseAddress.typed("HTTPS://Example.COM/A"))
        assertEquals("https://example.com", GlasshouseAddress.typed("https:example.com"))
        assertEquals("https://example.com", GlasshouseAddress.typed("https:////example.com"))
        assertEquals("https://example.com", GlasshouseAddress.typed("https:\\\\example.com"))
    }

    @Test
    fun theTextAroundTheAddressIsDropped() {
        assertEquals("https://example.com", GlasshouseAddress.typed("  example.com \n"))
        assertEquals("https://example.com", GlasshouseAddress.typed("exam\tple.com"))
    }

    @Test
    fun aPortIsNotMistakenForASchemeAndIsKeptAsTyped() {
        assertEquals("https://localhost:8080/app", GlasshouseAddress.typed("localhost:8080/app"))
        assertEquals("http://localhost:3000", GlasshouseAddress.typed("http://localhost:3000"))
        assertEquals("https://192.168.0.7:8443", GlasshouseAddress.typed("192.168.0.7:8443"))
        assertEquals("https://example.com:443", GlasshouseAddress.typed("https://example.com:443"))
    }

    @Test
    fun anIpv6HostKeepsItsBrackets() {
        assertEquals("https://[::1]:3000", GlasshouseAddress.typed("[::1]:3000"))
        assertEquals("http://[2001:DB8::1]", GlasshouseAddress.typed("http://[2001:DB8::1]"))
    }

    @Test
    fun whatFollowsTheHostIsKeptAsTyped() {
        assertEquals("https://example.com/a b", GlasshouseAddress.typed("example.com/a b"))
        assertEquals("https://example.com/?a[]=1&b=%", GlasshouseAddress.typed("example.com/?a[]=1&b=%"))
        assertEquals("https://example.com/\u0430", GlasshouseAddress.typed("example.com/\u0430"))
        assertEquals("https://M\u00fcnchen.de/\u00e4", GlasshouseAddress.typed("M\u00fcnchen.de/\u00e4"))
        assertEquals("https://example.com/a\\b", GlasshouseAddress.typed("example.com/a\\b"))
        assertEquals("https://example.com\\a b", GlasshouseAddress.typed("example.com\\a b"))
        assertEquals("https://example.com:99999", GlasshouseAddress.typed("https://example.com:99999"))
    }

    @Test
    fun aLoginBeforeTheHostIsNotTheHost() {
        assertEquals("https://user:pw@example.com", GlasshouseAddress.typed("https://user:pw@example.com"))
        assertEquals("https://a@b@example.com", GlasshouseAddress.typed("https://a@b@example.com"))
        assertNull(GlasshouseAddress.typed("https://user@"))
        assertNull(GlasshouseAddress.typed("https://user:pw@:80"))
    }

    @Test
    fun otherSchemesAreRefused() {
        val refused =
            listOf(
                "file:///etc/passwd",
                "FILE://localhost/etc/passwd",
                "javascript:alert(1)",
                " JavaScript:alert(1)",
                "java\tscript:alert(1)",
                "data:text/html,<p>hi</p>",
                "ftp://example.com/file",
                "mailto:someone@example.com",
                "tel:+15551234",
                "about:blank",
                "blob:https://example.com/1234",
                "view-source:https://example.com",
                "intent://scan/#Intent;scheme=zxing;end",
                "content://media/external/images",
                "ws://example.com/socket",
            )
        for (text in refused) {
            assertNull(GlasshouseAddress.typed(text), text)
        }
    }

    @Test
    fun textThatIsNotAnAddressIsRefused() {
        val refused =
            listOf(
                "",
                "   ",
                "https://",
                "http://:80",
                "hello world",
                "https://exa mple.com",
                "https://exa\u00a0mple.com",
                "https://exa\u0001mple.com",
                "https://?q=1",
                "https://#top",
            )
        for (text in refused) {
            assertNull(GlasshouseAddress.typed(text), text)
        }
    }

    @Test
    fun aLinkIsWebOnlyWhenItSaysSo() {
        assertTrue(GlasshouseAddress.isWeb("https://example.com/a"))
        assertTrue(GlasshouseAddress.isWeb("HTTP://example.com"))
        assertFalse(GlasshouseAddress.isWeb("example.com"))
        // `typed` takes this for a host and a port; as a link it is a phone number.
        assertFalse(GlasshouseAddress.isWeb("tel:5551234"))
        assertFalse(GlasshouseAddress.isWeb("file:///etc/passwd"))
        assertFalse(GlasshouseAddress.isWeb("javascript:alert(1)"))
        assertFalse(GlasshouseAddress.isWeb("about:blank"))
        assertFalse(GlasshouseAddress.isWeb(""))
        assertFalse(GlasshouseAddress.isWeb("https://"))
    }

    @Test
    fun theStartAddressComesFromTheConfiguration() {
        assertEquals("https://example.org/start", GlasshouseAddress.start("https://example.org/start"))
        assertEquals("https://example.org", GlasshouseAddress.start(" example.org "))
        assertEquals("http://example.org", GlasshouseAddress.start("http://example.org"))
    }

    @Test
    fun withoutAWebAddressInTheConfigurationThereIsNoStartAddress() {
        assertNull(GlasshouseAddress.start(null))
        assertNull(GlasshouseAddress.start(""))
        assertNull(GlasshouseAddress.start("   "))
        assertNull(GlasshouseAddress.start("file:///etc/passwd"))
        assertNull(GlasshouseAddress.start("not an address"))
    }
}
