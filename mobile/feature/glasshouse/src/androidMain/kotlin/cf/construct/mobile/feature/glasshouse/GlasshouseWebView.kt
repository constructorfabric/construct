package cf.construct.mobile.feature.glasshouse

import android.annotation.SuppressLint
import android.content.Context
import android.content.MutableContextWrapper
import android.graphics.Bitmap
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.http.SslError
import android.view.View
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.GeolocationPermissions
import android.webkit.PermissionRequest
import android.webkit.RenderProcessGoneDetail
import android.webkit.SslErrorHandler
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.util.concurrent.ConcurrentHashMap

/** What the web view sees, as the calls of the holder that wants to hear it. */
class GlasshouseWebEvents(
    val onPageStarted: (String) -> Unit,
    val onNavigated: (address: String, canGoBack: Boolean, canGoForward: Boolean) -> Unit,
    val onPageFinished: () -> Unit,
    val onPageFailed: (String, GlasshouseProblem) -> Unit,
)

/**
 * The phone's web view for the Glasshouse page, set up as the page needs it, and kept alive by whoever owns
 * this object (a view model) while the screen comes and goes: the page, what was typed in it and its history stay.
 * What the page opens, refuses and keeps is set out in the README, "The Glasshouse page on Android".
 *
 * Use it on the main thread. Call [attach] when the screen shows the web view and [detach] when it leaves.
 */
class GlasshouseWebView(
    context: Context,
    private val version: String,
    private val events: GlasshouseWebEvents,
) {
    private val appContext = context.applicationContext
    private val mutableView = MutableStateFlow(newWebView(appContext))

    /** The web view to show. It is replaced when its process stops, so watch it. */
    val view: StateFlow<WebView> = mutableView.asStateFlow()

    private val mutableProgress = MutableStateFlow(FULL)

    /** How much of the page has loaded, 0 to 100. */
    val progress: StateFlow<Int> = mutableProgress.asStateFlow()

    // The addresses the main page has had since the load began: as typed, and as the web view writes each request of
    // the page and each redirect of it, which is how a certificate error names the page. The web view asks for a
    // request off the main thread.
    private val mainPage: MutableSet<String> = ConcurrentHashMap.newKeySet()
    private var address = ""

    /** Carries out what the holder asks for. */
    fun execute(command: GlasshouseCommand) {
        val web = mutableView.value
        when (command) {
            is GlasshouseCommand.Open -> {
                mainPage.clear()
                mainPage += command.address
                address = command.address
                if (isOffline(appContext)) {
                    noNetwork(command.address)
                } else {
                    web.loadUrl(command.address)
                }
            }

            GlasshouseCommand.Back -> {
                web.goBack()
            }

            GlasshouseCommand.Forward -> {
                web.goForward()
            }

            GlasshouseCommand.Reload -> {
                if (isOffline(appContext)) noNetwork(address) else web.reload()
            }
        }
    }

    /**
     * Puts [web], which has to be the current [view], into [container], taking it out of the one it was in;
     * [hidden] keeps it off the screen. A web view that was replaced since is ignored.
     */
    fun attach(
        container: ViewGroup,
        web: WebView,
        hidden: Boolean,
    ) {
        if (web !== mutableView.value) return
        web.baseContext = container.context
        if (web.parent !== container) {
            (web.parent as? ViewGroup)?.removeView(web)
            container.removeAllViews()
            container.addView(web, ViewGroup.LayoutParams(MATCH, MATCH))
        }
        web.visibility = if (hidden) View.INVISIBLE else View.VISIBLE
    }

    /** Takes the web view out of its container; it keeps running. */
    fun detach() {
        val web = mutableView.value
        (web.parent as? ViewGroup)?.removeView(web)
        web.baseContext = appContext
    }

    /** Writes the cookies to the disk now, so a sign-in survives the app being closed. */
    fun flushCookies() {
        CookieManager.getInstance().flush()
    }

    fun destroy() {
        detach()
        mutableView.value.destroy()
    }

    // The context a web view shows dialogs and reads resources through: the screen's while it is on the screen, the
    // app's otherwise. Every web view here comes from [newWebView] with this wrapper, so the cast cannot fail.
    private var WebView.baseContext: Context
        get() = (context as MutableContextWrapper).baseContext
        set(value) {
            (context as MutableContextWrapper).baseContext = value
        }

    @SuppressLint("SetJavaScriptEnabled")
    private fun newWebView(base: Context): WebView {
        val web = WebView(MutableContextWrapper(base))
        with(web.settings) {
            javaScriptEnabled = true
            domStorageEnabled = true
            // Some of these are the web view's default already; they are written out so that what a page may reach
            // reads in one place.
            // Location is refused here and again in the chrome client: if one of the two is ever changed, the other
            // still stands.
            allowFileAccess = false
            allowContentAccess = false
            setGeolocationEnabled(false)
            setSupportMultipleWindows(false)
            mediaPlaybackRequiresUserGesture = true
            mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
            // Set before the first load, so the first request carries it too.
            userAgentString = GlasshouseUserAgent.of(userAgentString, version)
        }
        CookieManager.getInstance().setAcceptCookie(true)
        web.setDownloadListener { _, _, _, _, _ -> }
        web.webViewClient = PageClient()
        web.webChromeClient = PageChrome()
        return web
    }

    /**
     * A page is not asked for while the phone has no network: the web view would keep using a connection it had
     * open before the network went, and wait on it with no word for the person. The page says so at once instead.
     */
    private fun noNetwork(address: String) {
        events.onPageFailed(address, GlasshouseProblem.Offline)
    }

    /**
     * The main page goes to [url]: with no network it is not asked for, and the page says so. The address is
     * recorded as the main page's either way, because the certificate check reads it when the page fails.
     */
    private fun cancelWhenOffline(url: String): Boolean {
        mainPage += url
        val cancel = isOffline(appContext)
        if (cancel) noNetwork(url)
        return cancel
    }

    private fun report(
        web: WebView,
        url: String?,
    ) {
        val shown = url?.takeIf(GlasshouseAddress::isWeb) ?: return
        address = shown
        events.onNavigated(shown, web.canGoBack(), web.canGoForward())
    }

    /** The web view's process stopped: the page is lost, so make a new web view and say so. */
    private fun replace(dead: WebView) {
        val base = dead.baseContext
        (dead.parent as? ViewGroup)?.removeView(dead)
        dead.destroy()
        mutableView.value = newWebView(base)
        mutableProgress.value = FULL
        mainPage.clear()
        if (address.isNotEmpty()) events.onNavigated(address, false, false)
        events.onPageFailed(address, GlasshouseProblem.Crashed)
    }

    private inner class PageClient : WebViewClient() {
        override fun onPageStarted(
            view: WebView,
            url: String?,
            favicon: Bitmap?,
        ) {
            val started = url ?: return
            if (GlasshouseAddress.isWeb(started)) {
                mainPage += started
                address = started
                events.onPageStarted(started)
            }
        }

        override fun onPageFinished(
            view: WebView,
            url: String?,
        ) {
            report(view, url)
            events.onPageFinished()
            flushCookies()
        }

        override fun doUpdateVisitedHistory(
            view: WebView,
            url: String?,
            isReload: Boolean,
        ) {
            report(view, url)
        }

        // Called before the request goes out, so before any certificate error of it.
        override fun shouldInterceptRequest(
            view: WebView,
            request: WebResourceRequest,
        ): WebResourceResponse? {
            if (request.isForMainFrame) mainPage += request.url.toString()
            return null
        }

        override fun shouldOverrideUrlLoading(
            view: WebView,
            request: WebResourceRequest,
        ): Boolean {
            val url = request.url.toString()
            if (GlasshouseAddress.isWeb(url)) return request.isForMainFrame && cancelWhenOffline(url)
            // Nothing opens, not another app either. A redirect cancelled half way leaves the page it began on.
            if (request.isForMainFrame) {
                report(view, view.url)
                events.onPageFinished()
            }
            return true
        }

        override fun onReceivedError(
            view: WebView,
            request: WebResourceRequest,
            error: WebResourceError,
        ) {
            if (request.isForMainFrame) {
                events.onPageFailed(
                    request.url.toString(),
                    GlasshouseErrors.problemOf(error.errorCode, isOffline(appContext)),
                )
            }
        }

        override fun onReceivedSslError(
            view: WebView,
            handler: SslErrorHandler,
            error: SslError,
        ) {
            // A certificate the system does not trust is never accepted, for the page or anything on it.
            handler.cancel()
            if (error.url.trimEnd('/') in mainPage.map { it.trimEnd('/') }) {
                events.onPageFailed(error.url, GlasshouseProblem.Certificate)
            }
        }

        override fun onRenderProcessGone(
            view: WebView,
            detail: RenderProcessGoneDetail,
        ): Boolean {
            replace(view)
            return true
        }
    }

    private inner class PageChrome : WebChromeClient() {
        override fun onProgressChanged(
            view: WebView,
            newProgress: Int,
        ) {
            mutableProgress.value = newProgress
        }

        override fun onPermissionRequest(request: PermissionRequest) {
            request.deny()
        }

        override fun onGeolocationPermissionsShowPrompt(
            origin: String,
            callback: GeolocationPermissions.Callback,
        ) {
            callback.invoke(origin, false, false)
        }
    }

    private companion object {
        const val FULL = 100
        const val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
    }
}

/** What a phone's web view calls a failed load, in the kinds of problem the page tells apart. */
internal object GlasshouseErrors {
    /**
     * The problem for the web view's [errorCode]. The codes that depend on the network say [GlasshouseProblem.Offline]
     * when the phone has none; a certificate the system does not trust is reported by its own call, not by a code.
     */
    fun problemOf(
        errorCode: Int,
        offline: Boolean,
    ): GlasshouseProblem =
        when (errorCode) {
            WebViewClient.ERROR_HOST_LOOKUP -> if (offline) GlasshouseProblem.Offline else GlasshouseProblem.NotFound
            WebViewClient.ERROR_CONNECT -> if (offline) GlasshouseProblem.Offline else GlasshouseProblem.Refused
            WebViewClient.ERROR_TIMEOUT -> if (offline) GlasshouseProblem.Offline else GlasshouseProblem.Other
            else -> GlasshouseProblem.Other
        }
}

/**
 * Whether the phone has no network to ask for a page with. Stricter than the phone connection's check in
 * `Composition.kt` (any active network): a web page needs one that is meant to reach the internet, and the phone
 * connection only the local network.
 */
private fun isOffline(context: Context): Boolean {
    val manager = context.getSystemService(ConnectivityManager::class.java)
    val network = manager?.activeNetwork ?: return true
    val capabilities = manager.getNetworkCapabilities(network)
    return capabilities?.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) != true
}
