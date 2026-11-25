package cf.construct.mobile.core.connection

import cf.construct.mobile.core.chat.PhoneChannel
import cf.construct.mobile.core.chat.PhoneReplyKind
import kotlinx.coroutines.ensureActive
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlin.coroutines.CoroutineContext

/** The computer stopped answering in the middle of a call: the channel closed or the line was not an answer. */
class ComputerLost : Exception("The computer stopped answering.")

/**
 * One authenticated channel to the paired computer, for as long as the block of [ComputerAccess.reach] runs.
 * Its calls block on I/O, so the block runs where blocking is fine.
 */
interface ComputerChannel {
    /**
     * Which computer is on the other end: the fingerprint of its certificate, the one the pairing checked. It is
     * not the address, which can change, and it is not a secret.
     */
    val identity: String

    /**
     * Sends one JSON line and returns the object the computer answered. The text of an answer can hold what
     * the person keeps on the computer, so it is never logged and never part of an error.
     *
     * @throws ComputerLost when no answer comes, or the computer has unpaired this phone.
     * @throws kotlinx.coroutines.CancellationException when the caller was cancelled before the call.
     */
    fun ask(request: JsonObject): JsonObject
}

/** What a call to the computer came to: it ran, or the link said why it could not. */
sealed interface ComputerReach<out T> {
    data class Reached<T>(
        val value: T,
    ) : ComputerReach<T>

    /** Nothing ran to its end. [link] is never [ComputerLink.Connected]. */
    data class Missed(
        val link: ComputerLink,
    ) : ComputerReach<Nothing>
}

/** Runs calls on a channel to the paired computer. [PhoneConnection] is the one the app uses. */
interface ComputerAccess {
    /**
     * Opens a channel to the paired computer, proves the phone to it and runs [block] on it. The calls of one
     * block use one channel. The block does not run when no channel opens, and a [ComputerLost] inside it ends
     * it as [ComputerLink.Unreachable].
     */
    suspend fun <T> reach(block: (ComputerChannel) -> T): ComputerReach<T>
}

private class OpenChannel(
    override val identity: String,
    private val channel: PhoneChannel,
    private val context: CoroutineContext,
) : ComputerChannel {
    override fun ask(request: JsonObject): JsonObject {
        context.ensureActive()
        val reply = channel.exchange(request.toString())
        val answer =
            try {
                reply.line?.takeIf { reply.kind == PhoneReplyKind.Line }?.let {
                    Json.parseToJsonElement(
                        it,
                    ) as? JsonObject
                }
            } catch (_: SerializationException) {
                null
            }
        val unpaired = answer?.get("revoked") == JsonPrimitive(true)
        return answer?.takeUnless { unpaired } ?: throw ComputerLost()
    }
}

/** Proves the phone on an opened channel and runs [block] on it, or says why it did not run. */
internal fun <T> PhoneProtocol.runOn(
    saved: StoredPairing,
    channel: PhoneChannel,
    context: CoroutineContext,
    block: (ComputerChannel) -> T,
): ComputerReach<T> =
    when (authenticate(channel, saved)) {
        Probe.Connected -> {
            try {
                ComputerReach.Reached(block(OpenChannel(saved.pin, channel, context)))
            } catch (_: ComputerLost) {
                ComputerReach.Missed(ComputerLink.Unreachable)
            }
        }

        Probe.PhoneUpdate -> {
            ComputerReach.Missed(ComputerLink.PhoneUpdate)
        }

        Probe.ComputerUpdate -> {
            ComputerReach.Missed(ComputerLink.ComputerUpdate)
        }

        Probe.Revoked, Probe.Unreachable -> {
            ComputerReach.Missed(ComputerLink.Unreachable)
        }
    }
