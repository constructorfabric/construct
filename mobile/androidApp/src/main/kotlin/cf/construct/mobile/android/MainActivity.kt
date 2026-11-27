package cf.construct.mobile.android

import android.Manifest
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import cf.construct.mobile.core.chat.ChatCore
import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.database.CopyStore
import cf.construct.mobile.core.keys.ModelSettingsStore
import cf.construct.mobile.feature.dashboard.DashboardActions
import cf.construct.mobile.feature.dashboard.DashboardHolder
import cf.construct.mobile.feature.dashboard.DashboardScreen
import cf.construct.mobile.feature.glasshouse.GlasshouseScreen
import cf.construct.mobile.feature.memory.MemoryHolder
import cf.construct.mobile.feature.memory.MemoryScreen
import cf.construct.mobile.feature.settings.SettingsActions
import cf.construct.mobile.feature.settings.SettingsHolder
import cf.construct.mobile.feature.settings.SettingsScreen
import cf.construct.mobile.feature.status.ComputerOperation
import cf.construct.mobile.feature.status.ConnectionStatus
import cf.construct.mobile.feature.status.ConnectionStatusKind
import cf.construct.mobile.feature.status.FirstCopyScreen
import cf.construct.mobile.feature.status.SyncState

class MainActivity : ComponentActivity() {
    private val connection: ConnectionViewModel by viewModels {
        viewModelFactory {
            initializer {
                val app = application as ConstructApp
                ConnectionViewModel(app.phoneConnection, app.copy::removeComputerCopy)
            }
        }
    }
    private val localNetworkPermission =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            connection.holder.tryAgain()
            sync.holder.syncNow()
            if (granted) connection.clearOperation()
        }

    private val sync: SyncViewModel by viewModels {
        viewModelFactory { initializer { SyncViewModel((application as ConstructApp).syncer) } }
    }

    private val chat: ChatViewModel by viewModels {
        viewModelFactory {
            initializer {
                val app = application as ConstructApp
                ChatViewModel(app.construct, app.modelSettings)
            }
        }
    }

    // Made when the person first opens the page; the activity flushes its cookies and takes the page's text off the
    // clipboard only if it exists.
    private val glasshouseModel =
        viewModels<GlasshouseViewModel> {
            viewModelFactory {
                initializer {
                    val app = application as ConstructApp
                    GlasshouseViewModel(app, BuildConfig.GLASSHOUSE_START_ADDRESS, app.versionName)
                }
            }
        }

    private val glasshouseRestriction
        get() = if (glasshouseModel.isInitialized()) glasshouseModel.value.web.restriction else null

    private val memory: MemoryViewModel by viewModels {
        viewModelFactory { initializer { MemoryViewModel((application as ConstructApp).construct) } }
    }

    private val dashboard: DashboardViewModel by viewModels {
        viewModelFactory {
            initializer {
                val app = application as ConstructApp
                DashboardViewModel(app.construct, app.copy)
            }
        }
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
        setContent { ConstructTheme { AppContent() } }
    }

    @Composable
    private fun AppContent() {
        val state by memory.holder.state.collectAsState()
        val settingsState by settings.holder.state.collectAsState()
        val connectionState by connection.holder.state.collectAsState()
        val operation by connection.operation.collectAsState()
        val syncState by sync.holder.state.collectAsState()
        val dashboardState by dashboard.holder.state.collectAsState()
        // A sync that only brings the copy up to date changes no number, so the time of the update is read again.
        LaunchedEffect(syncState.lastSyncedAt) { dashboard.holder.refresh() }
        // A pairing that is done is followed by the first copy at once, not at the next check.
        LaunchedEffect(operation.paired) { if (operation.paired) sync.holder.syncNow() }
        // The first copy has a screen of its own over the whole app, until the copy is whole.
        val firstCopy =
            syncState.firstCopy?.takeIf { it.running && connectionState.kind == ConnectionStatusKind.Connected }
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Box(Modifier.fillMaxSize()) {
                // The app stays composed under the loading screen, which keeps the page the person was on. TalkBack
                // does not see it, and the loading screen takes the touches, so no tab can be reached.
                Box(if (firstCopy != null) Modifier.clearAndSetSemantics {} else Modifier) {
                    ChatRoute(
                        chat,
                        syncState,
                        onTryAgain = ::retryConnection,
                        computerContent = { page, open ->
                            ComputerPageContent(page, open, connectionState, syncState, operation)
                        },
                    ) { page, openComputer ->
                        when (page) {
                            Page.Memory -> {
                                MemoryScreen(state, onRefresh = memory.holder::refresh)
                            }

                            Page.Dashboard -> {
                                DashboardScreen(dashboardState, dashboard.actions)
                            }

                            Page.Settings -> {
                                SettingsScreen(settingsState, settings.actions) {
                                    ComputerSettingsRow(connectionState, openComputer)
                                }
                            }

                            else -> {
                                OtherPage(page, glasshouseModel)
                            }
                        }
                    }
                }
                if (firstCopy != null) {
                    // Back does nothing here: the pages under the loading screen must not go back unseen.
                    BackHandler {}
                    FirstCopyScreen(firstCopy)
                }
            }
        }
    }

    /** The Computer page and the two pages of pairing one step below it. */
    @Composable
    private fun ComputerPageContent(
        page: ComputerPage,
        open: (ComputerPage) -> Unit,
        connectionState: ConnectionStatus,
        syncState: SyncState,
        operation: ComputerOperation,
    ) {
        LaunchedEffect(page) { connection.clearOperation() }
        LaunchedEffect(operation.paired) {
            if (operation.paired && page != ComputerPage.Overview) open(ComputerPage.Overview)
        }
        when (page) {
            ComputerPage.Overview -> {
                ComputerScreen(
                    connectionState,
                    syncState,
                    operation,
                    open,
                    retry = ::retryConnection,
                    syncNow = sync.holder::syncNow,
                ) {
                    connection.unpair(::refreshAfterUnpair)
                }
            }

            ComputerPage.Manual -> {
                ManualPairingScreen(operation, connection::pairByCode, ::requestLocalNetwork)
            }

            ComputerPage.Scan -> {
                ScanPairingScreen(
                    operation,
                    connection::pairByQr,
                    connection::clearOperation,
                    ::requestLocalNetwork,
                ) { open(ComputerPage.Manual) }
            }
        }
    }

    override fun onStart() {
        super.onStart()
        connection.holder.setForeground(true)
        sync.holder.setForeground(true)
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) glasshouseRestriction?.rememberClipboard()
    }

    private fun refreshAfterUnpair() {
        memory.holder.refresh()
        chat.holder.refresh()
        // The notice of a computer that is no longer paired goes now, not at the next check.
        sync.holder.checkNow()
    }

    private fun retryConnection() {
        if (connection.holder.state.value.kind == ConnectionStatusKind.NetworkPermission &&
            !hasLocalNetworkPermission()
        ) {
            requestLocalNetwork()
        } else {
            connection.holder.tryAgain()
            sync.holder.syncNow()
        }
    }

    private fun requestLocalNetwork() {
        if (!hasLocalNetworkPermission()) {
            localNetworkPermission.launch(Manifest.permission.ACCESS_LOCAL_NETWORK)
        } else {
            connection.clearOperation()
        }
    }

    override fun onStop() {
        connection.holder.setForeground(false)
        // A turn of the screen stops and starts the activity, but the app does not leave: its sync goes on in the
        // view model, so a first copy that is under way is not started again from its first step.
        if (!isChangingConfigurations) sync.holder.setForeground(false)
        // A sign-in made in the page must be on the disk if the app is closed now.
        if (glasshouseModel.isInitialized()) glasshouseModel.value.web.flushCookies()
        // Restricted mode: the page's text does not go with the app to another app.
        glasshouseRestriction?.restoreClipboard()
        super.onStop()
    }
}

/** Keeps the Memory holder across a turn of the screen. */
internal class MemoryViewModel(
    construct: Construct,
) : ViewModel() {
    val holder = MemoryHolder(construct, viewModelScope)
}

/** Keeps the Dashboard holder across a turn of the screen. It reads only the copy on the phone. */
internal class DashboardViewModel(
    construct: Construct,
    copy: CopyStore,
) : ViewModel() {
    val holder = DashboardHolder(construct, copy::syncRecord, viewModelScope)
    val actions =
        DashboardActions(
            onSpan = holder::showSpan,
            onPrevious = holder::previous,
            onNext = holder::next,
            onToday = holder::showToday,
            onPick = holder::pick,
        )
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

/**
 * The pages that are not Memory, Dashboard or Settings: the Glasshouse page, and a placeholder for the rest. The
 * Glasshouse view model is made when its page is first shown, not before.
 */
@Composable
private fun OtherPage(
    page: Page,
    glasshouseModel: Lazy<GlasshouseViewModel>,
) {
    if (page == Page.Glasshouse) {
        val model = glasshouseModel.value
        val state by model.holder.state.collectAsState()
        GlasshouseScreen(state, model.web, model.actions)
    } else {
        PlaceholderPage(page)
    }
}
