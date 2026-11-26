package cf.construct.mobile.feature.status

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.res.stringResource
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeParseException
import java.time.format.FormatStyle
import java.util.Locale

/**
 * The time a copy was last brought up to date, in the words of the person's phone: the date, abbreviated, and the
 * time. [saved] is what the copy keeps (RFC 3339); it is null when the copy never was brought up to date, and it is
 * unreadable only if the copy was damaged, which shows the same as never.
 */
internal fun formatSyncTime(
    saved: String?,
    zone: ZoneId = ZoneId.systemDefault(),
    locale: Locale = Locale.getDefault(),
): String? {
    val moment =
        try {
            saved?.let(OffsetDateTime::parse)
        } catch (_: DateTimeParseException) {
            null
        }
    return moment?.let {
        DateTimeFormatter
            .ofLocalizedDateTime(FormatStyle.MEDIUM, FormatStyle.SHORT)
            .withLocale(locale)
            .format(it.atZoneSameInstant(zone))
    }
}

/** "Last sync <date>" for the Computer page, or null when the copy was never brought up to date. */
@Composable
fun lastSyncText(saved: String?): String? {
    val time = remember(saved) { formatSyncTime(saved) } ?: return null
    return stringResource(R.string.sync_last_sync, time)
}

@Composable
internal fun notReachableText(saved: String?): String {
    val time = remember(saved) { formatSyncTime(saved) }
    return if (time == null) {
        stringResource(R.string.sync_not_reachable)
    } else {
        stringResource(R.string.sync_not_reachable_last, time)
    }
}
