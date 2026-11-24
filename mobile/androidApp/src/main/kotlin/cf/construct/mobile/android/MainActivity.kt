package cf.construct.mobile.android

import android.Manifest
import android.content.pm.ApplicationInfo
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import cf.construct.mobile.core.chat.ChatCore
import cf.construct.mobile.core.connection.PhoneConnection
import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.keys.ModelSettingsStore
import cf.construct.mobile.feature.memory.MemoryHolder
import cf.construct.mobile.feature.memory.MemoryScreen
import cf.construct.mobile.feature.settings.SettingsActions
import cf.construct.mobile.feature.settings.SettingsHolder
import cf.construct.mobile.feature.settings.SettingsScreen
import cf.construct.mobile.feature.status.ConnectionHolder
import cf.construct.mobile.feature.status.ConnectionStatusKind

class MainActivity : ComponentActivity() {
    private val connection: ConnectionViewModel by viewModels {
        viewModelFactory {
            initializer {
                val app = application as ConstructApp
                val fixture =
                    applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0 &&
                        intent.getBooleanExtra("pairing-frame-test", false)
                val connection =
                    if (fixture) {
                        createPhoneConnection(
                            app,
                            PAIRING_FRAME_POLL_MILLIS,
                        )
                    } else {
                        app.phoneConnection
                    }
                ConnectionViewModel(connection)
            }
        }
    }
    private val localNetworkPermission =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { connection.holder.tryAgain() }

    private val chat: ChatViewModel by viewModels {
        viewModelFactory {
            initializer {
                val app = application as ConstructApp
                ChatViewModel(app.construct, app.modelSettings)
            }
        }
    }

    private val memory: MemoryViewModel by viewModels {
        viewModelFactory { initializer { MemoryViewModel((application as ConstructApp).construct) } }
    }

    private val settings: SettingsViewModel by viewModels {
        viewModelFactory {
            initializer {
                val app = application as ConstructApp
                SettingsViewModel(app.modelSettings, app.chatCore, chat.holder::refreshModel)
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        setContent {
            ConstructTheme {
                val state by memory.holder.state.collectAsState()
                val settingsState by settings.holder.state.collectAsState()
                val connectionState by connection.holder.state.collectAsState()
                Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    ChatRoute(chat, connectionState, onTryAgain = ::retryConnection) { page ->
                        when (page) {
                            Page.Memory -> MemoryScreen(state, onRefresh = memory.holder::refresh)
                            Page.Settings -> SettingsScreen(settingsState, settings.actions)
                            else -> PlaceholderPage(page)
                        }
                    }
                }
            }
        }
    }

    override fun onStart() {
        super.onStart()
        connection.holder.setForeground(true)
    }

    private fun retryConnection() {
        if (connection.holder.state.value.kind == ConnectionStatusKind.NetworkPermission &&
            !hasLocalNetworkPermission()
        ) {
            localNetworkPermission.launch(Manifest.permission.ACCESS_LOCAL_NETWORK)
        } else {
            connection.holder.tryAgain()
        }
    }

    override fun onStop() {
        connection.holder.setForeground(false)
        super.onStop()
    }

    private companion object {
        const val PAIRING_FRAME_POLL_MILLIS = 120_000L
    }
}

internal class ConnectionViewModel(
    connection: PhoneConnection,
) : ViewModel() {
    val holder = ConnectionHolder(connection, viewModelScope)
}

/** Keeps the Memory holder across a turn of the screen. */
internal class MemoryViewModel(
    construct: Construct,
) : ViewModel() {
    val holder = MemoryHolder(construct, viewModelScope)
}

/** Keeps the model setup across a turn of the screen. */
internal class SettingsViewModel(
    store: ModelSettingsStore,
    core: ChatCore,
    onCommit: () -> Unit,
) : ViewModel() {
    val holder = SettingsHolder(store, core, viewModelScope, onCommit = onCommit)
    val actions =
        SettingsActions(
            onChange = holder::change,
            onSave = holder::save,
            onTest = holder::test,
            onRemoveKey = holder::removeKey,
            onTryAgain = holder::load,
        )
}
