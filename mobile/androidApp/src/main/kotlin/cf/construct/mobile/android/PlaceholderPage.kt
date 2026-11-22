package cf.construct.mobile.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import cf.construct.mobile.core.designsystem.Spacing

/** Stands in for a page that has no feature yet: its icon and title. */
@Composable
internal fun PlaceholderPage(
    page: Page,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier.fillMaxSize(),
        verticalArrangement = Arrangement.spacedBy(Spacing.M.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(
            page.icon,
            contentDescription = null,
            Modifier.size(LocalControlSizes.current.iconLarge),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(stringResource(page.title), color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
