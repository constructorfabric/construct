package cf.construct.mobile.android

import android.app.Application
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import cf.construct.mobile.feature.glasshouse.GlasshouseActions
import cf.construct.mobile.feature.glasshouse.GlasshouseHolder
import cf.construct.mobile.feature.glasshouse.GlasshouseWebEvents
import cf.construct.mobile.feature.glasshouse.GlasshouseWebView
import kotlinx.coroutines.launch

/**
 * Keeps the Glasshouse page and its web view for as long as the activity's screen lives, so the page, what was
 * typed in it and its history stay while the person is on another page of the app. It is made when the person first
 * opens the page, and then the web view opens the start address of the build, if there is one.
 */
internal class GlasshouseViewModel(
    application: Application,
    startAddress: String?,
    version: String,
) : ViewModel() {
    val holder = GlasshouseHolder(startAddress)
    val web =
        GlasshouseWebView(
            application,
            version,
            GlasshouseWebEvents(
                onPageStarted = holder::pageStarted,
                onNavigated = holder::navigated,
                onPageFinished = holder::pageFinished,
                onPageFailed = holder::pageFailed,
            ),
        )
    val actions =
        GlasshouseActions(
            onGo = {
                holder.go(it)
                !holder.state.value.addressRefused
            },
            onEdited = holder::addressEdited,
            onReload = holder::reload,
            onBack = holder::back,
        )

    init {
        // Once for the web view, before the commands are collected.
        holder.webViewStarted()
        viewModelScope.launch { holder.commands.collect(web::execute) }
    }

    override fun onCleared() {
        web.destroy()
    }
}
