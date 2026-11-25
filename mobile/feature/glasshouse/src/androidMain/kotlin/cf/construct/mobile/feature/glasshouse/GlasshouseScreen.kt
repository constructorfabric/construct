package cf.construct.mobile.feature.glasshouse

import android.widget.FrameLayout
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import cf.construct.mobile.core.designsystem.Spacing

private const val REFUSED = "Only http and https addresses open here."
private const val NO_START_ADDRESS = "No start address is set. Type an address above to open a page."

/**
 * The Glasshouse page: the address bar, and under it the web page. When the page cannot open, the page area says
 * why with a Reload button, in the place of the page; a refused address is told under the bar. It loads nothing
 * itself: the web view is the one it is given, and what the person does goes out through [actions].
 *
 * The system back goes back in the page when it can.
 */
@Composable
fun GlasshouseScreen(
    state: GlasshouseState,
    webView: GlasshouseWebView,
    actions: GlasshouseActions,
    modifier: Modifier = Modifier,
) {
    val progress by webView.progress.collectAsState()
    val problem = state.problem
    val notice = state.noStartAddress && state.address.isEmpty()
    val text =
        when {
            problem != null -> problemText(problem)
            notice -> NO_START_ADDRESS
            else -> null
        }
    BackHandler(enabled = state.canGoBack, onBack = actions.onBack)
    Column(modifier.fillMaxSize().imePadding()) {
        GlasshouseAddressBar(
            state,
            progress,
            actions,
            Modifier.padding(horizontal = Spacing.L.dp, vertical = Spacing.S.dp),
        )
        if (state.addressRefused) {
            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            GlasshouseMessage(REFUSED, Modifier.padding(Spacing.L.dp), error = true)
        }
        Box(Modifier.weight(1f).fillMaxWidth()) {
            GlasshousePageView(webView, hidden = text != null)
            if (text != null) {
                Box(
                    Modifier
                        .fillMaxSize()
                        .background(MaterialTheme.colorScheme.surfaceVariant)
                        .pointerInput(Unit) { detectTapGestures { } }
                        .padding(Spacing.L.dp),
                ) {
                    GlasshouseMessage(
                        text,
                        error = problem != null,
                        onReload =
                            actions.onReload.takeIf {
                                problem !=
                                    null
                            },
                    )
                }
            }
        }
    }
}

/** The web view in a frame that gives it up when the screen leaves, so the next screen can take it again. */
@Composable
private fun GlasshousePageView(
    webView: GlasshouseWebView,
    hidden: Boolean,
    modifier: Modifier = Modifier,
) {
    val view by webView.view.collectAsState()
    AndroidView(
        factory = { FrameLayout(it) },
        modifier = modifier.fillMaxSize(),
        // Reading the view here runs this again when the web view is replaced.
        update = { webView.attach(it, view, hidden) },
        onRelease = { webView.detach() },
    )
}

private fun problemText(problem: GlasshouseProblem): String =
    when (problem) {
        GlasshouseProblem.NotFound -> "The page cannot open: no site was found at this address."
        GlasshouseProblem.Refused -> "The page cannot open: the site refused the connection."
        GlasshouseProblem.Offline -> "The page cannot open: there is no network connection."
        GlasshouseProblem.Certificate -> "The page cannot open: the site's certificate is not trusted."
        GlasshouseProblem.Crashed -> "The page stopped working."
        GlasshouseProblem.Other -> "The page cannot open."
    }
