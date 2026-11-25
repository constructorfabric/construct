package cf.construct.mobile.feature.glasshouse

/** What the address bar shows while the person is not typing in it. */
object GlasshouseDisplay {
    /**
     * The host of [address] with its port, as a browser shows the page it is on: `https://user@example.com:8443/a?b#c`
     * shows `example.com:8443`. Anything that is not a web address is shown as it is.
     */
    fun host(address: String): String = GlasshouseAddress.host(address) ?: address
}
