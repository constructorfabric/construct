package cf.construct.mobile.android

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import cf.construct.mobile.feature.status.SyncNoticeRow
import cf.construct.mobile.feature.status.SyncState

/**
 * The frame every page sits in: the top app bar with the page title, the navigation bar, and above the page's
 * own content the notice of the sync ("Syncing with your computer.", "not reachable"). When all is fine the
 * notice is not there. [content] draws the selected page.
 */
@Composable
fun AppFrame(
    sync: SyncState,
    onTryAgain: () -> Unit,
    selected: Page,
    onSelect: (Page) -> Unit,
    modifier: Modifier = Modifier,
    pageActions: @Composable RowScope.() -> Unit = {},
    computerContent: @Composable (ComputerPage, (ComputerPage) -> Unit) -> Unit,
    content: @Composable (Page, () -> Unit) -> Unit,
) {
    var computer by rememberSaveable { mutableStateOf<ComputerPage?>(null) }
    val detail = computer
    val goBack = { computer = if (detail == ComputerPage.Overview) null else ComputerPage.Overview }
    BackHandler(enabled = detail != null, onBack = goBack)
    Scaffold(
        modifier = modifier,
        containerColor = MaterialTheme.colorScheme.background,
        topBar = {
            // The Glasshouse page is a browser: its address bar takes the place of the title, and it has nothing
            // to sync.
            if (selected != Page.Glasshouse) {
                TopAppBar(
                    title = { Text(detail?.title ?: stringResource(selected.title)) },
                    actions = { if (detail == null) pageActions() },
                    navigationIcon = {
                        if (detail != null) {
                            IconButton(onClick = goBack) {
                                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                            }
                        }
                    },
                    colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.surface),
                )
            }
        },
        bottomBar = { if (detail == null) PageBar(selected, onSelect) },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            // The Glasshouse page and the pages one step down (the Computer page) tell nothing of the sync.
            if (detail == null && selected != Page.Glasshouse) {
                SyncNoticeRow(sync.notice, sync.lastSyncedAt, onTryAgain, Modifier.fillMaxWidth())
            }
            Box(Modifier.fillMaxWidth().weight(1f)) {
                if (detail == null) {
                    content(selected) { computer = ComputerPage.Overview }
                } else {
                    computerContent(detail) { computer = it }
                }
            }
        }
    }
}

enum class ComputerPage(
    val title: String,
) {
    Overview("Computer"),
    Manual("Pair with a computer"),
    Scan("Scan QR code"),
}

@Composable
private fun PageBar(
    selected: Page,
    onSelect: (Page) -> Unit,
) {
    Column {
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
        NavigationBar(containerColor = MaterialTheme.colorScheme.surface, tonalElevation = 0.dp) {
            for (page in Page.entries) {
                val active = page == selected
                NavigationBarItem(
                    selected = active,
                    onClick = { onSelect(page) },
                    icon = { Icon(page.icon, contentDescription = null) },
                    // The active page is told apart by the pill and a heavier label, not by colour alone.
                    label = {
                        Text(
                            stringResource(page.title),
                            fontWeight = if (active) FontWeight.SemiBold else FontWeight.Medium,
                            maxLines = 1,
                        )
                    },
                    colors =
                        NavigationBarItemDefaults.colors(
                            selectedIconColor = MaterialTheme.colorScheme.onSecondaryContainer,
                            selectedTextColor = MaterialTheme.colorScheme.onBackground,
                            indicatorColor = MaterialTheme.colorScheme.secondaryContainer,
                            unselectedIconColor = MaterialTheme.colorScheme.onSurfaceVariant,
                            unselectedTextColor = MaterialTheme.colorScheme.onSurfaceVariant,
                        ),
                )
            }
        }
    }
}
