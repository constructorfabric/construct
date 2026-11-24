package cf.construct.mobile.android

import cf.construct.mobile.core.connection.PHONE_POLL_INTERVAL_MILLIS

/** How often the app checks the computer. Only debug builds let a test change it. */
internal object PhonePolling {
    val intervalMillis: Long get() = PHONE_POLL_INTERVAL_MILLIS
}
