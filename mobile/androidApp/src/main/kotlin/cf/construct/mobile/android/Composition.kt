package cf.construct.mobile.android

import android.content.Context
import cf.construct.mobile.core.chat.ChatCore
import cf.construct.mobile.core.chat.nativeChatCore
import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.database.CopyStore
import cf.construct.mobile.core.database.PhoneConstruct
import cf.construct.mobile.core.database.androidCopyDriver
import cf.construct.mobile.core.keys.ModelSettingsStore
import cf.construct.mobile.core.keys.androidModelSettingsStore
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
