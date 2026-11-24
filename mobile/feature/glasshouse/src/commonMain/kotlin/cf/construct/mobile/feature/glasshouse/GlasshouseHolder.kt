package cf.construct.mobile.feature.glasshouse

import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update

/**
 * Holds what both phones share about the Glasshouse page, as one [GlasshouseState]. Plain common Kotlin,
 * so the Android screen and the iPhone screen watch the same class; call everything on the main thread.
 *
 * The web view owns the page and its history: the person's actions become [commands] for it to carry out,
 * and what it sees is reported back with the `page…` and [navigated] functions. A web view that starts
 * says so with [webViewStarted]; collecting [commands] again does not.
 *
 * @param configuredStartAddress what the app's build configuration holds, or null; see
 *   [GlasshouseAddress.start]. With none, the state says [GlasshouseState.noStartAddress] and the person
 *   types one.
 */
class GlasshouseHolder(
    configuredStartAddress: String?,
) {
    private val commandChannel = Channel<GlasshouseCommand>(Channel.UNLIMITED)
    private val mutableState = MutableStateFlow(GlasshouseState())
    val state: StateFlow<GlasshouseState> = mutableState.asStateFlow()

    /**
     * The commands for the web view, in order, kept until it takes them. One collector at a time: stop the
     * old one before another starts, since a collector that is still waiting takes the next command, and
     * one cancelled just as a command reached it loses that command. A collector that starts after the old
     * one stopped (a screen that redraws) continues from what is queued.
     */
    val commands: Flow<GlasshouseCommand> = commandChannel.receiveAsFlow()

    init {
        val start = GlasshouseAddress.start(configuredStartAddress)
        mutableState.update { it.copy(address = start.orEmpty(), noStartAddress = start == null) }
    }

    /**
     * A web view starts, a new one with no history: call it once for each, as it is made. What the earlier
     * web view had not taken yet is dropped, and the new one is told to open the current address, the start
     * address at first. Stop collecting [commands] for the earlier web view before this.
     */
    fun webViewStarted() {
        while (commandChannel.tryReceive().isSuccess) {
            // The earlier web view is gone; the new one starts from the current address.
        }
        mutableState.update { it.copy(canGoBack = false, canGoForward = false) }
        val address = mutableState.value.address
        if (address.isNotEmpty()) go(address)
    }

    /**
     * The person typed [text] and asked to go. A web address opens, with `https://` added when it has no
     * scheme; anything else is refused and the state says so ([GlasshouseState.addressRefused]).
     */
    fun go(text: String) {
        val address = GlasshouseAddress.typed(text)
        if (address == null) {
            mutableState.update { it.copy(addressRefused = true) }
        } else {
            mutableState.update { it.copy(address = address, loading = true, problem = null, addressRefused = false) }
            commandChannel.trySend(GlasshouseCommand.Open(address))
        }
    }

    /** The person edited the address they were typing: a refusal of the earlier text no longer applies. */
    fun addressEdited() {
        mutableState.update { it.copy(addressRefused = false) }
    }

    /** One page back, when there is one. The page it goes to replaces a failed one. */
    fun back() {
        if (mutableState.value.canGoBack) {
            mutableState.update { it.copy(problem = null, addressRefused = false) }
            commandChannel.trySend(GlasshouseCommand.Back)
        }
    }

    /** One page forward, when there is one. The page it goes to replaces a failed one. */
    fun forward() {
        if (mutableState.value.canGoForward) {
            mutableState.update { it.copy(problem = null, addressRefused = false) }
            commandChannel.trySend(GlasshouseCommand.Forward)
        }
    }

    /**
     * Loads the page again. After a [GlasshouseProblem] it opens the address that failed, since a failed
     * or crashed page cannot be reloaded as it is; the problem is cleared. With no page yet, nothing
     * happens.
     */
    fun reload() {
        val now = mutableState.value
        if (now.address.isEmpty()) return
        if (now.problem != null) {
            go(now.address)
        } else {
            mutableState.update { it.copy(loading = true, addressRefused = false) }
            commandChannel.trySend(GlasshouseCommand.Reload)
        }
    }

    /**
     * The web view started loading the page at [address]. Whatever was wrong with the page before is over.
     * An address that is not a web one (`about:blank`) is ignored: it is not a page of the person's.
     */
    fun pageStarted(address: String) {
        if (GlasshouseAddress.isWeb(address)) {
            mutableState.update { it.copy(address = address, loading = true, problem = null) }
        }
    }

    /**
     * The web view shows the page at [address], with this history. Call it whenever the address or the
     * history changes, an in-page link included, and when a load ends. A report about an address that is
     * not a web one is ignored.
     */
    fun navigated(
        address: String,
        canGoBack: Boolean,
        canGoForward: Boolean,
    ) {
        if (GlasshouseAddress.isWeb(address)) {
            mutableState.update { it.copy(address = address, canGoBack = canGoBack, canGoForward = canGoForward) }
        }
    }

    /** The web view stopped loading, whether the page opened or not: a problem stays until the next load. */
    fun pageFinished() {
        mutableState.update { it.copy(loading = false) }
    }

    /**
     * The page at [address] cannot be shown, for [problem]; a web view whose process stopped reports
     * [GlasshouseProblem.Crashed] with the address it had. Report the main page only, not a picture or a
     * script of it, and not a load that another load replaced. The problem shows even when [address] is not a
     * web address; the address shown stays the last web one.
     */
    fun pageFailed(
        address: String,
        problem: GlasshouseProblem,
    ) {
        mutableState.update {
            val shown = address.takeIf(GlasshouseAddress::isWeb) ?: it.address
            it.copy(address = shown, loading = false, problem = problem)
        }
    }
}
