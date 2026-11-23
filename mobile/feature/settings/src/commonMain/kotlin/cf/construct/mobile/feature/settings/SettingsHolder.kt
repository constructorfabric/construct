package cf.construct.mobile.feature.settings

import cf.construct.mobile.core.chat.ChatCore
import cf.construct.mobile.core.chat.ModelTestResult
import cf.construct.mobile.core.chat.desktopReasons
import cf.construct.mobile.core.keys.ModelSettingsException
import cf.construct.mobile.core.keys.ModelSettingsStore
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

/** The reasons of a Test that belong to the tool check, so the first line has passed when one of them comes. */
private val toolReasons = listOf("tools refused", "no tool call", "bad tool call", "wrong answer")

/**
 * Holds the model setup: reads and writes [store], runs the Test through [core], and turns it all into
 * one [SettingsState]. Plain common Kotlin, so both apps watch the same class. Storage and the Test wait
 * for the disk and the network, so they run on [io]; the state changes on the thread of [scope], the main
 * one.
 */
class SettingsHolder(
    private val store: ModelSettingsStore,
    private val core: ChatCore,
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher = Dispatchers.Default,
) {
    private val mutableState = MutableStateFlow(SettingsState())
    private val writes = Mutex()
    private var revision = 0L
    val state: StateFlow<SettingsState> = mutableState.asStateFlow()

    init {
        load()
    }

    /** Reads the stored setting; after a failure it is the "Try again". */
    fun load() {
        mutableState.update { it.copy(loading = true, unavailable = null) }
        scope.launch {
            val read = attempt { store.read() }
            mutableState.update { current ->
                read.fold(
                    {
                        SettingsState(
                            loading = false,
                            address = it.address,
                            model = it.model,
                            keyIsSet = it.keyIsSet,
                            pendingWrites = current.pendingWrites,
                        )
                    },
                    { current.copy(loading = false, unavailable = it) },
                )
            }
        }
    }

    /** The person typed [value] in [field]. */
    fun change(
        field: SettingsField,
        value: String,
    ) = edit {
        when (field) {
            SettingsField.Address -> it.copy(address = value, addressMissing = false)
            SettingsField.ModelName -> it.copy(model = value, modelMissing = false)
            SettingsField.Key -> it.copy(keyEntry = value)
        }
    }

    /** Saves the text, and the key when one was typed; the typed key leaves the screen state at once. */
    fun save() {
        val now = mutableState.value
        val addressMissing = now.address.isBlank()
        val modelMissing = now.model.isBlank()
        if (addressMissing || modelMissing) {
            mutableState.update { it.copy(addressMissing = addressMissing, modelMissing = modelMissing, saved = false) }
            return
        }
        val typed = now.keyEntry
        if (typed.isNotEmpty()) invalidateCheck()
        mutableState.update { it.copy(keyEntry = "", failure = null, saved = false) }
        writeSettings {
            val done =
                attempt {
                    store.saveText(now.address.trim(), now.model.trim())
                    if (typed.isNotEmpty()) store.saveKey(typed)
                }
            mutableState.update { current ->
                done.fold(
                    { current.copy(saved = true, keyIsSet = current.keyIsSet || typed.isNotEmpty()) },
                    { current.copy(failure = it, keyEntry = typed) },
                )
            }
        }
    }

    fun removeKey() {
        invalidateCheck()
        mutableState.update { it.copy(failure = null, saved = false) }
        writeSettings {
            val done = attempt { store.removeKey() }
            mutableState.update { current ->
                done.fold(
                    { current.copy(keyIsSet = false, keyEntry = "", check = SettingsCheck.None) },
                    { current.copy(failure = it) },
                )
            }
        }
    }

    private fun writeSettings(write: suspend () -> Unit) {
        mutableState.update { it.copy(pendingWrites = it.pendingWrites + 1) }
        // Enter the mutex in action order, before dispatching work to storage.
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            try {
                writes.withLock { write() }
            } finally {
                mutableState.update { it.copy(pendingWrites = it.pendingWrites - 1) }
            }
        }
    }

    /** Tests the setting as it is on the screen: the typed key, or else the saved one. */
    fun test() {
        val now = mutableState.value
        if (now.pendingWrites > 0 || now.testing) return
        if (!now.canTest) {
            mutableState.update { it.copy(addressMissing = it.address.isBlank(), modelMissing = it.model.isBlank()) }
            return
        }
        val testedRevision = revision
        mutableState.update {
            it.copy(
                testing = true,
                failure = null,
                saved = false,
                check = SettingsCheck.Shown(CheckLine(CheckStatus.Checking), CheckLine(CheckStatus.Checking)),
            )
        }
        scope.launch {
            val result =
                attempt {
                    val key = now.keyEntry.ifEmpty { store.readKey().orEmpty() }
                    core.testModel(now.address.trim(), now.model.trim(), key)
                }
            mutableState.update { current ->
                if (testedRevision != revision) return@update current
                result.fold(
                    { current.copy(testing = false, check = shown(it)) },
                    { current.copy(testing = false, check = SettingsCheck.None, failure = it) },
                )
            }
        }
    }

    private fun edit(change: (SettingsState) -> SettingsState) {
        invalidateCheck()
        mutableState.update { change(it).copy(saved = false) }
    }

    private fun invalidateCheck() {
        revision++
        mutableState.update { it.copy(testing = false, check = SettingsCheck.None) }
    }

    private suspend fun <T> attempt(call: () -> T): Result<T, String> =
        withContext(io) {
            try {
                Result.Done(call())
            } catch (e: ModelSettingsException) {
                // The message of a storage failure is fixed text; nothing else is shown to the person.
                Result.Failed(e.message.orEmpty())
            }
        }
}

private fun shown(result: ModelTestResult): SettingsCheck.Shown =
    when (result) {
        is ModelTestResult.Works -> {
            SettingsCheck.Shown(CheckLine(CheckStatus.Works), CheckLine(CheckStatus.Works))
        }

        is ModelTestResult.Failed -> {
            val reason = plain(result.reason)
            if (toolReasons.any { result.reason.startsWith(it) }) {
                SettingsCheck.Shown(CheckLine(CheckStatus.Works), CheckLine(CheckStatus.Failed, reason))
            } else {
                SettingsCheck.Shown(CheckLine(CheckStatus.Failed, reason), CheckLine(CheckStatus.Skipped))
            }
        }
    }

/** The desktop's reason without its label: "model not found: …" shows the text after the colon. */
private fun plain(reason: String) = if (reason in desktopReasons.values) reason.substringAfter(": ") else reason

/** A finished call: its value, or the plain reason it failed. */
private sealed interface Result<out T, out E> {
    data class Done<T>(
        val value: T,
    ) : Result<T, Nothing>

    data class Failed<E>(
        val reason: E,
    ) : Result<Nothing, E>
}

private inline fun <T, E, R> Result<T, E>.fold(
    done: (T) -> R,
    failed: (E) -> R,
): R =
    when (this) {
        is Result.Done -> done(value)
        is Result.Failed -> failed(reason)
    }
