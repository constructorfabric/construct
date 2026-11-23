package cf.construct.mobile.android

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.Spacing

/**
 * The frame every page sits in: the top app bar with the page title, the connection line under it
 * and the navigation bar. [content] draws the selected page.
 */
@Composable
fun AppFrame(
    connection: ConnectionState,
    selected: Page,
    onSelect: (Page) -> Unit,
    modifier: Modifier = Modifier,
    pageActions: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit = {},
    content: @Composable (Page) -> Unit,
) {
    Scaffold(
        modifier = modifier,
        containerColor = MaterialTheme.colorScheme.background,
        topBar = {
            Column {
                TopAppBar(
                    title = { Text(stringResource(selected.title)) },
                    actions = pageActions,
                    colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.surface),
                )
                if (selected != Page.Chat) ConnectionLine(connection)
            }
        },
        bottomBar = { PageBar(selected, onSelect) },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) { content(selected) }
    }
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

/** One line under the top bar: a dot and the connection state in words. */
@Composable
internal fun ConnectionLine(
    state: ConnectionState,
    modifier: Modifier = Modifier,
) {
    val connected = state == ConnectionState.Connected
    val dot = if (connected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline
    Row(
        modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surface)
            .padding(horizontal = Spacing.L.dp, vertical = Spacing.S.dp),
        horizontalArrangement = Arrangement.spacedBy(Spacing.S.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(LocalControlSizes.current.dot).background(dot, CircleShape))
        Text(
            stringResource(if (connected) R.string.connection_connected else R.string.connection_not_connected),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.labelMedium,
        )
    }
}
