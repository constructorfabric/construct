package cf.construct.mobile.feature.status

import cf.construct.mobile.core.connection.ComputerLink
import cf.construct.mobile.core.connection.LinkSnapshot
import cf.construct.mobile.core.connection.PhoneConnection
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * The frame and the Computer page can watch this without access to pairing credentials. The monitor runs only
 * while the app is in the foreground.
 */
class ConnectionHolder internal constructor(
    watch: suspend ((LinkSnapshot) -> Unit) -> Unit,
    scope: CoroutineScope,
) {
    constructor(connection: PhoneConnection, scope: CoroutineScope) : this(connection::monitor, scope)

    private val request = MutableStateFlow(WatchRequest())
    private val stateValue = MutableStateFlow(ConnectionStatus(ConnectionStatusKind.Checking))
    val state = stateValue.asStateFlow()

    init {
        scope.launch {
            request.collectLatest { wanted ->
                if (wanted.active) {
                    watch { stateValue.value = ConnectionStatus.from(it) }
                }
            }
        }
    }

    fun setForeground(active: Boolean) {
        request.update { it.copy(active = active) }
    }

    /** Checks the computer now: a new request restarts the collector, which restarts the monitor. */
    fun tryAgain() {
        request.update { it.copy(attempt = it.attempt + 1) }
    }

    /**
     * Nothing reads [attempt]. A request with a new value is what makes the flow emit again, and so what
     * makes [collectLatest] start the monitor again.
     */
    private data class WatchRequest(
        val active: Boolean = false,
        val attempt: Int = 0,
    )
}

enum class ConnectionStatusKind(
    val title: String,
    val detail: String? = null,
    val canRetry: Boolean = false,
) {
    Checking("Checking connection…"),
    NotPaired("Not paired"),
    Connected("Connected"),
    NoNetwork("The computer is not reachable", "This phone has no network connection.", true),
    NetworkPermission("Local network access is needed", "Allow access to connect to your computer.", true),
    Unreachable(
        "The computer is not reachable",
        "The computer may be asleep or off, on another network, or have a new address " +
            "after a network change or router restart.",
        true,
    ),
    PhoneUpdate("Update this app to connect to the computer"),
    ComputerUpdate("Update Construct on the computer to connect"),
    StorageError("The saved pairing could not be opened", canRetry = true),
}

data class ConnectionStatus(
    val kind: ConnectionStatusKind,
    val lastContactMillis: Long = 0,
    val endpoint: String? = null,
) {
    companion object {
        internal fun from(snapshot: LinkSnapshot): ConnectionStatus {
            val kind =
                when (snapshot.link) {
                    ComputerLink.NotPaired -> ConnectionStatusKind.NotPaired
                    ComputerLink.Connected -> ConnectionStatusKind.Connected
                    ComputerLink.NoNetwork -> ConnectionStatusKind.NoNetwork
                    ComputerLink.NetworkPermission -> ConnectionStatusKind.NetworkPermission
                    ComputerLink.Unreachable -> ConnectionStatusKind.Unreachable
                    ComputerLink.PhoneUpdate -> ConnectionStatusKind.PhoneUpdate
                    ComputerLink.ComputerUpdate -> ConnectionStatusKind.ComputerUpdate
                    ComputerLink.StorageError -> ConnectionStatusKind.StorageError
                }
            return ConnectionStatus(kind, snapshot.lastContactMillis, snapshot.endpoint)
        }
    }
}
