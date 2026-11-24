package cf.construct.mobile.feature.status

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import cf.construct.mobile.core.designsystem.ColorRole
import cf.construct.mobile.core.designsystem.Colors
import java.text.DateFormat
import java.util.Date

/** The dot of a state: green when connected, red when the computer cannot be reached, grey while unknown. */
@Composable
fun ConnectionStatusKind.dotColor(): Color =
    when (this) {
        ConnectionStatusKind.Connected -> {
            val colors = if (isSystemInDarkTheme()) Colors.dark else Colors.light
            Color(colors.getValue(ColorRole.SUCCESS))
        }

        ConnectionStatusKind.Checking, ConnectionStatusKind.NotPaired -> {
            MaterialTheme.colorScheme.outline
        }

        else -> {
            MaterialTheme.colorScheme.error
        }
    }

/** The time of the last contact in the words both the connection line and the Computer page use. */
@Composable
fun lastContactText(millis: Long): String? =
    millis.takeIf { it > 0 }?.let {
        stringResource(R.string.connection_last_contact, DateFormat.getDateTimeInstance().format(Date(it)))
    }
