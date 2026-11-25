package cf.construct.mobile.feature.glasshouse

import android.webkit.WebViewClient
import kotlin.test.Test
import kotlin.test.assertEquals

class GlasshouseErrorsTest {
    @Test
    fun aNameThatDoesNotResolveIsNotFound() {
        assertEquals(
            GlasshouseProblem.NotFound,
            GlasshouseErrors.problemOf(WebViewClient.ERROR_HOST_LOOKUP, offline = false),
        )
    }

    @Test
    fun aConnectionThatFailsIsRefused() {
        assertEquals(
            GlasshouseProblem.Refused,
            GlasshouseErrors.problemOf(WebViewClient.ERROR_CONNECT, offline = false),
        )
    }

    @Test
    fun theCodesThatNeedTheNetworkSayOfflineWhenThePhoneHasNone() {
        for (code in listOf(
            WebViewClient.ERROR_HOST_LOOKUP,
            WebViewClient.ERROR_CONNECT,
            WebViewClient.ERROR_TIMEOUT,
        )) {
            assertEquals(GlasshouseProblem.Offline, GlasshouseErrors.problemOf(code, offline = true))
        }
    }

    @Test
    fun anythingElseIsOtherWhetherOrNotThePhoneIsOnline() {
        for (offline in listOf(true, false)) {
            assertEquals(GlasshouseProblem.Other, GlasshouseErrors.problemOf(WebViewClient.ERROR_UNKNOWN, offline))
            assertEquals(
                GlasshouseProblem.Other,
                GlasshouseErrors.problemOf(WebViewClient.ERROR_FAILED_SSL_HANDSHAKE, offline),
            )
            assertEquals(
                GlasshouseProblem.Other,
                GlasshouseErrors.problemOf(WebViewClient.ERROR_UNSAFE_RESOURCE, offline),
            )
        }
        assertEquals(GlasshouseProblem.Other, GlasshouseErrors.problemOf(WebViewClient.ERROR_TIMEOUT, offline = false))
    }
}
