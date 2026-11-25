package cf.construct.mobile.feature.glasshouse

import kotlin.test.Test
import kotlin.test.assertEquals

class GlasshouseDisplayTest {
    @Test
    fun theHostAndItsPortAreShown() {
        assertEquals("example.test", GlasshouseDisplay.host("https://example.test/garden"))
        assertEquals("example.test:8443", GlasshouseDisplay.host("https://example.test:8443/a?b=1#c"))
        assertEquals("10.0.2.2:8443", GlasshouseDisplay.host("https://10.0.2.2:8443/"))
        assertEquals("[::1]:3000", GlasshouseDisplay.host("http://[::1]:3000/x"))
    }

    @Test
    fun aLoginBeforeTheHostIsNotShown() {
        assertEquals("example.test", GlasshouseDisplay.host("https://user:pw@example.test/"))
        assertEquals("example.test", GlasshouseDisplay.host("https://a@b@example.test"))
    }

    @Test
    fun aHostWithNothingAfterItIsShownAsIs() {
        assertEquals("example.test", GlasshouseDisplay.host("https://example.test"))
        assertEquals("example.test", GlasshouseDisplay.host("https://example.test?q=1"))
    }

    @Test
    fun whatIsNotAWebAddressIsShownAsItIs() {
        assertEquals("", GlasshouseDisplay.host(""))
        assertEquals("ftp://example.test/herbs", GlasshouseDisplay.host("ftp://example.test/herbs"))
        assertEquals("about:blank", GlasshouseDisplay.host("about:blank"))
    }
}
