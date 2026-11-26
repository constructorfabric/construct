package cf.construct.mobile.android

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.material3.Text
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.centerX
import androidx.compose.ui.test.click
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import cf.construct.mobile.feature.status.FirstCopy
import cf.construct.mobile.feature.status.FirstCopyScreen
import cf.construct.mobile.feature.status.SyncNotice
import cf.construct.mobile.feature.status.SyncNoticeRow
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** What the sync screens do for a touch and for a screen reader, one screen at a time and without a computer. */
class SyncScreensTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun theLoadingScreenTakesEveryTouchSoNoneReachesThePageUnderIt() {
        var touches = 0
        var topInset = 0
        compose.setContent {
            ConstructTheme {
                topInset = WindowInsets.safeDrawing.getTop(LocalDensity.current)
                Box(Modifier.fillMaxSize()) {
                    Text("Under", Modifier.fillMaxSize().clickable { touches++ })
                    FirstCopyScreen(FirstCopy(running = true))
                }
            }
        }
        // The text under the loading screen is in the tree and has a click, so only the loading screen stops it.
        compose.onNodeWithText("Under").performClick()
        compose.waitForIdle()
        assertEquals(0, touches, "a touch went through the loading screen")
        // The scroll area of the loading screen does not reach the strip under the status bar, so only the screen's
        // own touch handler takes a touch there.
        assertTrue(topInset > 0, "this window has no strip under the status bar, so the test cannot see the handler")
        compose.onNodeWithText("Under").performTouchInput { click(Offset(centerX, topInset / 2f)) }
        compose.waitForIdle()
        assertEquals(0, touches, "a touch went through the strip under the status bar")
        compose.onNodeWithTag("first-copy").assertExists()
    }

    @Test
    fun syncedJustNowIsAPoliteLiveRegion() {
        show(SyncNotice.JustSynced)
        compose.onNodeWithText("Up to date. Synced just now.").assertExists()
        compose.onAllNodes(politeLiveRegion).assertCountEquals(1)
    }

    @Test
    fun syncingIsNotALiveRegion() {
        show(SyncNotice.Syncing)
        compose.onNodeWithText("Syncing with your computer.").assertExists()
        compose.onAllNodes(politeLiveRegion).assertCountEquals(0)
    }

    @Test
    fun theComputerNotReachableIsNotALiveRegion() {
        show(SyncNotice.NotReachable)
        compose.onNodeWithText("Try again").assertExists()
        compose.onAllNodes(politeLiveRegion).assertCountEquals(0)
    }

    private fun show(notice: SyncNotice) {
        compose.setContent { ConstructTheme { SyncNoticeRow(notice, lastSyncedAt = null, onTryAgain = {}) } }
    }

    private val politeLiveRegion = SemanticsMatcher.expectValue(SemanticsProperties.LiveRegion, LiveRegionMode.Polite)
}
