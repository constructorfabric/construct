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

/** Safe to publish. The address, certificate pin and credential are kept in the connection. */
data class LinkSnapshot(
    val link: ComputerLink,
    val lastContactMillis: Long,
)

class PhoneConnection internal constructor(
    private val store: PairingSecret,
    private val transport: PhoneTransport,
    private val networkAvailable: () -> Boolean,
    version: Int = PHONE_PROTOCOL,
    private val dispatcher: CoroutineDispatcher = Dispatchers.IO,
    private val pollInterval: Duration = PHONE_POLL_INTERVAL_MILLIS.milliseconds,
    private val now: () -> Long = { Clock.System.now().toEpochMilliseconds() },
    private val localAccess: () -> Boolean = { true },
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

    private suspend fun pair(
        offer: PairingOffer,
        name: String,
    ): PairResult =
        withContext(dispatcher) {
            pairing.withLock {
                if (!networkAvailable()) return@withLock PairResult.NoNetwork
                if (!localAccess()) return@withLock PairResult.NetworkPermission
                val attempt = protocol.pair(offer, name)
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
                try {
                    watchPairing(expected, publish)
                } catch (_: ModelSettingsException) {
                    publish(LinkSnapshot(ComputerLink.StorageError, 0))
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
                    withContext(NonCancellable) { clearRevokedPairing(expected) }
                    return
                }
                currentCoroutineContext().ensureActive()
                contact.flush()
                val state =
                    when (result) {
                        Probe.PhoneUpdate -> ComputerLink.PhoneUpdate
                        Probe.ComputerUpdate -> ComputerLink.ComputerUpdate
                        else -> unavailableReason()
                    }
                publish(LinkSnapshot(state, contact.latest))
                delay(pollInterval)
            }
        } finally {
            withContext(NonCancellable) { contact.flush() }
        }
    }

    private suspend fun clearRevokedPairing(expected: Int) {
        storage.withLock {
            if (revision.value == expected) {
                store.clear()
                revision.value++
            }
        }
    }

    private fun unavailableReason() =
        when {
            !networkAvailable() -> ComputerLink.NoNetwork
            !localAccess() -> ComputerLink.NetworkPermission
            else -> ComputerLink.Unreachable
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
            publish(LinkSnapshot(ComputerLink.Connected, contact.latest))
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

        suspend fun flush() {
            if (latest == savedAt) return
            storage.withLock {
                if (revision.value != expected) return
                store.savePairing(pairing.copy(lastContactMillis = latest))
                savedAt = latest
            }
        }
    }

    private companion object {
        val SAVE_INTERVAL = 30.seconds
    }
}

const val PHONE_POLL_INTERVAL_MILLIS = 5000L

fun defaultPhoneConnection(
    store: PairingSecret,
    networkAvailable: () -> Boolean,
    protocolVersion: Int = PHONE_PROTOCOL,
    localAccess: () -> Boolean = { true },
    pollIntervalMillis: Long = PHONE_POLL_INTERVAL_MILLIS,
): PhoneConnection =
    PhoneConnection(
        store,
        nativePhoneTransport(),
        networkAvailable,
        protocolVersion,
        pollInterval = pollIntervalMillis.milliseconds,
        localAccess = localAccess,
    )
