package cf.construct.mobile.android

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.os.Build
import cf.construct.mobile.core.chat.ChatCore
import cf.construct.mobile.core.chat.nativeChatCore
import cf.construct.mobile.core.connection.PHONE_POLL_INTERVAL_MILLIS
import cf.construct.mobile.core.connection.defaultPhoneConnection
import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.database.CopyStore
import cf.construct.mobile.core.database.PhoneConstruct
import cf.construct.mobile.core.database.androidCopyDriver
import cf.construct.mobile.core.keys.ModelSettingsStore
import cf.construct.mobile.core.keys.androidModelSettingsStore
import cf.construct.mobile.core.keys.androidPairingSecret
import kotlinx.coroutines.Dispatchers

/**
 * The one place the app's objects are built. To run the app on [cf.construct.mobile.core.construct.FakeConstruct]
 * instead of the phone's copy, change [createConstruct] and nothing else.
 */
internal fun createConstruct(
    context: Context,
    modelSettings: ModelSettingsStore,
): Construct {
    val store = CopyStore(androidCopyDriver(context))
    seedCopy(store)
    return PhoneConstruct(store, modelSettings, Dispatchers.IO)
}

/** The provider setting encrypted under an Android Keystore key. */
internal fun createModelSettingsStore(context: Context): ModelSettingsStore = androidModelSettingsStore(context)

internal fun createChatCore(): ChatCore = nativeChatCore()

internal fun createPhoneConnection(
    context: Context,
    pollIntervalMillis: Long = PHONE_POLL_INTERVAL_MILLIS,
) = defaultPhoneConnection(
    androidPairingSecret(context),
    networkAvailable = { context.getSystemService(ConnectivityManager::class.java)?.activeNetwork != null },
    localAccess = { context.hasLocalNetworkPermission() },
    pollIntervalMillis = pollIntervalMillis,
)

internal fun Context.hasLocalNetworkPermission() =
    Build.VERSION.SDK_INT < LOCAL_NETWORK_PERMISSION_API ||
        checkSelfPermission(Manifest.permission.ACCESS_LOCAL_NETWORK) == PackageManager.PERMISSION_GRANTED

private const val LOCAL_NETWORK_PERMISSION_API = 37
