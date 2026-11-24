package cf.construct.mobile.feature.glasshouse

/** Why the web page cannot be shown. Each phone maps the error codes of its own web view to one of these. */
enum class GlasshouseProblem {
    /** No site answers at the address. */
    NotFound,

    /** The site refused the connection. */
    Refused,

    /** The phone has no network connection. */
    Offline,

    /** The site's certificate is not trusted. */
    Certificate,

    /** The web view's process stopped. */
    Crashed,

    /** Any other failure, among them a plain `http` page that the phone blocks. */
    Other,
}

/** Everything the Glasshouse page shows, in one immutable value. */
data class GlasshouseState(
    /** The address the page is on, or the one it is going to. Empty before the first one. */
    val address: String = "",
    val loading: Boolean = false,
    val canGoBack: Boolean = false,
    val canGoForward: Boolean = false,
    /** True when the app's configuration gives no start address, so the person types one. */
    val noStartAddress: Boolean = false,
    /** Why the page cannot be shown, or null. */
    val problem: GlasshouseProblem? = null,
    /** True after the person typed an address that is not `http` or `https`, until they edit or open another. */
    val addressRefused: Boolean = false,
)

/** What the holder asks the phone's web view to do. The view does it and reports back to the holder. */
sealed interface GlasshouseCommand {
    /** Load [address], a web address as [GlasshouseAddress.typed] gives it. */
    data class Open(
        val address: String,
    ) : GlasshouseCommand

    data object Back : GlasshouseCommand

    data object Forward : GlasshouseCommand

    data object Reload : GlasshouseCommand
}
