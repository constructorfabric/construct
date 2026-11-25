package cf.construct.mobile.feature.glasshouse

/**
 * The rules for the address the Glasshouse page opens: only `http` and `https` addresses open, whoever
 * asks (the person typing, a link, a redirect, the app's configuration).
 *
 * This is the gate, not a URL parser: an address needs one of those two schemes and a host, and the rest
 * is kept as typed for the web view to read. A phone that cannot build its own URL type from an address
 * (the iPhone's `URL(string:)` gives nil for some) encodes it there, where the real system can be asked.
 */
object GlasshouseAddress {
    /**
     * The web address a person typed, or null when it is not one that opens. An address with no scheme
     * gets `https://`, so `example.com` opens `https://example.com`; `localhost:8080` is a host and a
     * port, not a scheme. Any scheme but `http` and `https` (`file:`, `javascript:`, `data:`, `mailto:`)
     * gives null, and so does an address with no host or with a space in the host.
     */
    fun typed(raw: String): String? {
        val text = clean(raw)
        return if (text.isEmpty()) null else parse(if (hasScheme(text)) text else "https://$text")
    }

    /**
     * Whether [address] is a web address as it stands: it has to say `http` or `https` itself. Use it for
     * an address the web view is about to go to, a link or a redirect. Nothing is guessed here (`tel:123`
     * is not a host and a port), unlike [typed].
     */
    fun isWeb(address: String): Boolean = parse(clean(address)) != null

    /**
     * The host of a web [address] with its port and without its login, as the rest of this object reads the host
     * (`https://user@example.com:8443/a?b#c` gives `example.com:8443`), or null when it is not a web address.
     */
    fun host(address: String): String? = parse(clean(address))?.let { hostOf(it.substringAfter("://")) }

    /**
     * The start address out of what the app's build configuration holds: [typed] of it, or null when it
     * is missing, empty or not a web address. There is no built-in address to fall back to.
     */
    fun start(configured: String?): String? = configured?.let(::typed)
}

private val schemePrefix = Regex("^([A-Za-z][A-Za-z0-9+.-]*):")

// `host:8080` reads like a scheme to the pattern above; a port is digits and then the end or a path.
private val portAfterHost = Regex("^:\\d+(?:[/?#]|$)")
private val webSchemes = setOf("http", "https")

private fun clean(raw: String): String = raw.filterNot { it == '\t' || it == '\n' || it == '\r' }.trim()

private fun hasScheme(text: String): Boolean {
    val scheme = schemePrefix.find(text)?.groupValues?.get(1) ?: return false
    return !portAfterHost.containsMatchIn(text.substring(scheme.length))
}

private fun parse(text: String): String? {
    val scheme = text.substringBefore(':', "").lowercase().takeIf { it in webSchemes } ?: return null
    // Slashes after the scheme are optional and either way round, as in a browser's address bar.
    val rest = text.substring(scheme.length + 1).trimStart('/', '\\')
    return if (hasHost(rest)) "$scheme://$rest" else null
}

/** The host is what stands between the login (up to the last `@`) and the path, query or fragment. */
private fun hostOf(rest: String): String = rest.takeWhile { it !in "/\\?#" }.substringAfterLast('@')

private fun hasHost(rest: String): Boolean {
    val host = hostOf(rest)
    return host.isNotEmpty() && !host.startsWith(":") && host.none { it.isISOControl() || it.isWhitespace() }
}
