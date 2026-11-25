package cf.construct.mobile.android

import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assume.assumeNotNull
import org.junit.Rule
import org.junit.Test

/**
 * A sign-in made in the page is still there after the app is closed and opened again. The runner starts this twice with
 * the app force-stopped in between (`glasshouseRestart` is `signin`, then `check`); a run with no such argument skips
 * it.
 */
class GlasshouseRestartTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    @Test
    fun aSignInIsStillThereAfterTheAppRestarts() {
        val phase = InstrumentationRegistry.getArguments().getString("glasshouseRestart")
        val server = TestServer.fromArguments()
        assumeNotNull("only the restart run sets these", phase, server)
        val page = PageDriver(compose)
        page.openGlasshouse()
        when (phase) {
            "signin" -> {
                page.forgetCookies()
                page.go(checkNotNull(server).trusted + "/signin")
                page.waitForPage("Signed in")
            }

            "check" -> {
                page.go(checkNotNull(server).trusted + "/account")
                page.waitForPage("Signed in as Sam")
                page.waitForPage("Remember cookie: yes")
                page.forgetCookies()
            }

            else -> {
                error("Unknown restart phase: $phase")
            }
        }
    }
}
