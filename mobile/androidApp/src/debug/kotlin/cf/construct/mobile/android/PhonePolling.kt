package cf.construct.mobile.android

import cf.construct.mobile.core.connection.PHONE_POLL_INTERVAL_MILLIS

/**
 * How often the app checks the computer. An instrumented test sets [intervalMillis] before the app's one
 * connection is built, so that only the Try again button can bring a lost link back inside the test.
 */
internal object PhonePolling {
    @Volatile
    var intervalMillis: Long = PHONE_POLL_INTERVAL_MILLIS
}
