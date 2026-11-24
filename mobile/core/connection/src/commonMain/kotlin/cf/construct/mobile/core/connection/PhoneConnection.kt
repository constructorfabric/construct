package cf.construct.mobile.core.connection

import cf.construct.mobile.core.chat.PhoneChannel
import cf.construct.mobile.core.chat.PhoneTransport
import cf.construct.mobile.core.chat.nativePhoneTransport
import cf.construct.mobile.core.keys.ModelSettingsException
import cf.construct.mobile.core.keys.PairingSecret
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.IO
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlin.time.Clock
import kotlin.time.Duration
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds

enum class ComputerLink {
    NotPaired,
    Connected,
    NoNetwork,
    NetworkPermission,
    Unreachable,
    PhoneUpdate,
    ComputerUpdate,
    StorageError,
}

/**
 * Safe to publish. The endpoint names the paired computer; the certificate pin and the credential are kept
 * in the connection. The endpoint is null while no pairing is read.
 */
data class LinkSnapshot(
    val link: ComputerLink,
    val lastContactMillis: Long,
    val endpoint: String? = null,
)

/**
 * One instance per app: it owns the revision that tells [monitor] the pairing changed, so a second instance
 * on the same store would not hear that the first one paired again. Call [monitor] from one coroutine at a
 * time; a second call would run a second connection to the computer.
 */
class PhoneConnection internal constructor(
    private val store: PairingSecret,
    private val transport: PhoneTransport,
    private val networkAvailable: () -> Boolean,
    version: Int = PHONE_PROTOCOL,
    private val dispatcher: CoroutineDispatcher = Dispatchers.IO,
    private val pollInterval: Duration = PHONE_POLL_INTERVAL_MILLIS.milliseconds,
    private val now: () -> Long = { Clock.System.now().toEpochMilliseconds() },
    private val localAccess: () -> Boolean = { true },
    private val localNetworkDenied: (ComputerAddress) -> Boolean = { false },
) {
    private val protocol = PhoneProtocol(transport, version)
    private val pairing = Mutex()
    private val storage = Mutex()
    private val revision = MutableStateFlow(0)

    suspend fun pairByCode(
        address: String,
        code: String,
        name: String,
    ): PairResult {
        val parsed = ComputerAddress.parse(address, transport::isNumericAddress) ?: return PairResult.InvalidAddress
        val offer = PairingOffer.code(parsed, code)
        return if (offer == null) PairResult.InvalidCode else pair(offer, name)
    }

    suspend fun pairByQr(
        payload: String,
        name: String,
    ): PairResult {
        val offer = PairingOffer.qr(payload, transport::isNumericAddress) ?: return PairResult.InvalidQr
        return pair(offer, name)
    }

    /**
     * Removes the computer's copy on the phone and the pairing together, and tells [monitor]. Both go while no
     * other pairing step runs. A pairing that cannot be removed throws, and the copy is already gone.
     */
    suspend fun unpair(removeComputerCopy: () -> Unit) =
        withContext(dispatcher + NonCancellable) {
            pairing.withLock {
                storage.withLock {
                    removeComputerCopy()
                    store.clear()
                    revision.value++
                }
            }
        }

    private suspend fun pair(
        offer: PairingOffer,
        name: String,
    ): PairResult =
        withContext(dispatcher) {
            pairing.withLock {
                if (!networkAvailable()) return@withLock PairResult.NoNetwork
                if (!localAccess()) return@withLock PairResult.NetworkPermission
                val attempt = protocol.pair(offer, name)
                if (attempt.result == PairResult.Unreachable && localNetworkDenied(offer.address)) {
                    return@withLock PairResult.NetworkPermission
                }
                val granted = attempt.pairing ?: return@withLock attempt.result
                saveNewPairing(granted)
            }
        }

    private suspend fun saveNewPairing(granted: StoredPairing): PairResult =
        try {
            storage.withLock {
                store.savePairing(granted.copy(lastContactMillis = now()))
                revision.value++
            }
            PairResult.Paired
        } catch (_: ModelSettingsException) {
            PairResult.StorageError
        }

    /** Watch only for as long as the caller keeps this coroutine alive. */
    suspend fun monitor(publish: (LinkSnapshot) -> Unit): Unit =
        withContext(dispatcher) {
            revision.collectLatest { expected ->
                // An unreadable record is shown and read again later: the next pairing replaces it, and a
                // record that was only busy opens again.
                var watching = true
                while (watching) {
                    try {
                        watchPairing(expected, publish)
                        watching = false
                    } catch (_: ModelSettingsException) {
                        publish(LinkSnapshot(ComputerLink.StorageError, 0))
                        delay(pollInterval)
                    }
                }
            }
        }

    private suspend fun watchPairing(
        expected: Int,
        publish: (LinkSnapshot) -> Unit,
    ) {
        val saved = storage.withLock { store.loadPairing(transport::isNumericAddress) }
        if (saved == null) {
            publish(LinkSnapshot(ComputerLink.NotPaired, 0))
            return
        }
        val contact = LastContact(saved, expected)
        try {
            while (revision.value == expected) {
                val result = checkConnection(saved, contact, publish)
                if (result == Probe.Revoked) {
                    clearRevokedPairing(expected, publish)
                    return
                }
                currentCoroutineContext().ensureActive()
                contact.flush()
                // Inline, not a helper: with unpair a helper would bring this class to detekt's limit of 11 functions.
                val state =
                    when {
                        result == Probe.PhoneUpdate -> ComputerLink.PhoneUpdate
                        result == Probe.ComputerUpdate -> ComputerLink.ComputerUpdate
                        !networkAvailable() -> ComputerLink.NoNetwork
                        !localAccess() || localNetworkDenied(saved.address) -> ComputerLink.NetworkPermission
                        else -> ComputerLink.Unreachable
                    }
                // The local network check blocks for up to a second and does not notice a cancel, so a poll that was
                // cancelled meanwhile (an unpair, a new pairing, the monitor stopping) must not publish its state.
                currentCoroutineContext().ensureActive()
                publish(contact.snapshot(state))
                delay(pollInterval)
            }
        } finally {
            withContext(NonCancellable) { contact.flush() }
        }
    }

    /**
     * The computer revoked this phone, so the record must go. A failed removal is shown as a storage error
     * and tried again: the computer answers an unknown token with silence, so the revocation is not read twice.
     */
    private suspend fun clearRevokedPairing(
        expected: Int,
        publish: (LinkSnapshot) -> Unit,
    ) {
        var cleared = false
        while (!cleared) {
            try {
                withContext(NonCancellable) {
                    storage.withLock {
                        if (revision.value == expected) {
                            store.clear()
                            revision.value++
                        }
                    }
                }
                cleared = true
            } catch (_: ModelSettingsException) {
                publish(LinkSnapshot(ComputerLink.StorageError, 0))
                delay(pollInterval)
            }
        }
    }

    private suspend fun checkConnection(
        saved: StoredPairing,
        contact: LastContact,
        publish: (LinkSnapshot) -> Unit,
    ): Probe {
        if (!networkAvailable() || !localAccess()) return Probe.Unreachable
        return protocol.open(saved)?.use { channel ->
            val auth = protocol.authenticate(channel, saved)
            if (auth == Probe.Connected) ping(channel, contact, publish) else auth
        } ?: Probe.Unreachable
    }

    private suspend fun ping(
        channel: PhoneChannel,
        contact: LastContact,
        publish: (LinkSnapshot) -> Unit,
    ): Probe {
        var result = protocol.ping(channel)
        while (result == Probe.Connected) {
            contact.latest = maxOf(contact.latest, now())
            currentCoroutineContext().ensureActive()
            if (contact.latest - contact.savedAt >= SAVE_INTERVAL.inWholeMilliseconds) contact.flush()
            publish(contact.snapshot(ComputerLink.Connected))
            delay(pollInterval)
            result = protocol.ping(channel)
        }
        return result
    }

    private inner class LastContact(
        private val pairing: StoredPairing,
        private val expected: Int,
    ) {
        var latest = pairing.lastContactMillis
        var savedAt = latest
            private set

        fun snapshot(link: ComputerLink) = LinkSnapshot(link, latest, pairing.address.endpoint)

        /**
         * The time of the last contact is only a hint to the person. A write that fails leaves the old time in
         * the record and is tried again at the next flush; it never stops the checks of the computer.
         */
        suspend fun flush() {
            if (latest == savedAt) return
            storage.withLock {
                if (revision.value == expected && trySave()) savedAt = latest
            }
        }

        private fun trySave(): Boolean =
            try {
                store.savePairing(pairing.copy(lastContactMillis = latest))
                true
            } catch (_: ModelSettingsException) {
                false
            }
    }

    private companion object {
        val SAVE_INTERVAL = 30.seconds
    }
}

const val PHONE_POLL_INTERVAL_MILLIS = 5000L

/** The connection the app shares; see [PhoneConnection] for why there is one. */
fun defaultPhoneConnection(
    store: PairingSecret,
    networkAvailable: () -> Boolean,
    protocolVersion: Int = PHONE_PROTOCOL,
    localAccess: () -> Boolean = { true },
    pollIntervalMillis: Long = PHONE_POLL_INTERVAL_MILLIS,
    localNetworkDenied: (String, Int) -> Boolean = { _, _ -> false },
): PhoneConnection =
    PhoneConnection(
        store,
        nativePhoneTransport(),
        networkAvailable,
        protocolVersion,
        pollInterval = pollIntervalMillis.milliseconds,
        localAccess = localAccess,
        localNetworkDenied = { localNetworkDenied(it.host, it.port) },
    )
