package cf.construct.mobile.feature.glasshouse

/** The User-Agent the page sends: the web view's own, with the app's token `Construct/<version>` put last. */
object GlasshouseUserAgent {
    private const val TOKEN = "Construct"

    /**
     * [base] is the web view's own User-Agent and [version] the app's own version. The app's own tokens
     * (any `Construct/…`, and any token that ends in `/<version>`) are taken out first, so the token is
     * never repeated, and `Construct/<version>` goes last. The `Chrome/…` and `Safari/…` tokens stay: sites
     * read them. Set the result before the first load, so the first request carries it too.
     */
    fun of(
        base: String,
        version: String,
    ): String {
        require(version.isNotEmpty() && version.none { it.isWhitespace() }) {
            "the app version must be one word"
        }
        val kept =
            base
                .split(' ')
                .filter { it.isNotEmpty() && !it.startsWith("$TOKEN/") && !it.endsWith("/$version") }
        return (kept + "$TOKEN/$version").joinToString(" ")
    }
}
