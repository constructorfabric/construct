package cf.construct.mobile.android

import androidx.annotation.StringRes
import androidx.compose.ui.graphics.vector.ImageVector

/** The five pages of the app, in the order of the navigation bar. */
enum class Page(
    @StringRes val title: Int,
    val icon: ImageVector,
) {
    Chat(R.string.page_chat, PageIcons.chat),
    Memory(R.string.page_memory, PageIcons.memory),
    Dashboard(R.string.page_dashboard, PageIcons.dashboard),
    Glasshouse(R.string.page_glasshouse, PageIcons.glasshouse),
    Settings(R.string.page_settings, PageIcons.settings),
}
