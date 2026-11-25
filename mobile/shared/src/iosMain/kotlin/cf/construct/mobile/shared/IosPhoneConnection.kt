package cf.construct.mobile.shared

import cf.construct.mobile.core.connection.PHONE_POLL_INTERVAL_MILLIS
import cf.construct.mobile.core.connection.PHONE_PROTOCOL
import cf.construct.mobile.core.connection.PhoneConnection
import cf.construct.mobile.core.connection.defaultPhoneConnection
import cf.construct.mobile.core.database.CopyStore
import cf.construct.mobile.core.keys.PairingSecret

/**
 * The connection of the iPhone app. A pairing records its computer in [copy] before it is saved, and the call stays
 * in Kotlin, where a copy that cannot be written fails the pairing as a storage error. It must not be a Swift
 * closure that calls the store: an exception that Kotlin throws without `@Throws` ends the app when it reaches Swift.
 */
fun iosPhoneConnection(
    copy: CopyStore,
    store: PairingSecret,
    networkAvailable: () -> Boolean,
    protocolVersion: Int = PHONE_PROTOCOL,
    localAccess: () -> Boolean = { true },
    pollIntervalMillis: Long = PHONE_POLL_INTERVAL_MILLIS,
    localNetworkDenied: (String, Int) -> Boolean = { _, _ -> false },
): PhoneConnection =
    defaultPhoneConnection(
        store,
        networkAvailable,
        copy::adoptComputer,
        protocolVersion,
        localAccess,
        pollIntervalMillis,
        localNetworkDenied,
    )
