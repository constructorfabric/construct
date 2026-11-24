package cf.construct.mobile.core.chat

/** Only typed-code authentication may open without a pin. Calls block on I/O. */
interface PhoneTransport {
    fun isNumericAddress(host: String): Boolean

    fun open(
        host: String,
        port: Int,
        pin: String?,
    ): PhoneChannel?

    fun proof(
        keyHex: String,
        fingerprint: String,
        agent: Boolean,
    ): String?
}

class PhoneIdentityMismatch : Exception("The computer’s certificate does not match the pairing.")

class PhoneProtocolMismatch : Exception("The computer does not support this typed pairing protocol.")

interface PhoneChannel : AutoCloseable {
    val fingerprint: String

    /** A line, a deliberate TLS close with no line, or loss of the connection. Needs a channel opened with a pin. */
    fun exchange(line: String): PhoneReply

    /** Authenticates the computer and returns its result on this channel. */
    fun pairCode(
        code: String,
        version: Int,
        name: String,
    ): PhoneReply
}

enum class PhoneReplyKind { Line, Closed, Unavailable, Refused }

/** The line can contain credentials. Use a fixed description when inspected. */
data class PhoneReply(
    val kind: PhoneReplyKind,
    val line: String? = null,
) {
    override fun toString() = kind.name
}

fun nativePhoneTransport(): PhoneTransport = NativePhoneTransport

internal expect object NativePhoneTransport : PhoneTransport

// Status codes from construct_chat.h. The same number means different things for different calls.
internal const val NATIVE_OK = 0

/** `cf_phone_open`: the certificate does not match the pin. */
internal const val NATIVE_OPEN_PIN_MISMATCH = 2

/** `cf_phone_exchange` and `cf_phone_pair_code`: the computer closed the channel without a line. */
internal const val NATIVE_REPLY_CLOSED = 2

/** `cf_phone_pair_code`: the computer refused the code. */
internal const val NATIVE_CODE_REFUSED = 3

/** `cf_phone_open` without a pin: the TLS service does not offer the typed pairing protocol. */
internal const val NATIVE_COMPUTER_UPDATE = 4

/** The result of `cf_phone_open` that is not a channel: an exception for a mismatch, else null. */
internal fun openFailure(code: Int): PhoneChannel? =
    when (code) {
        NATIVE_OPEN_PIN_MISMATCH -> throw PhoneIdentityMismatch()
        NATIVE_COMPUTER_UPDATE -> throw PhoneProtocolMismatch()
        else -> null
    }

/** The reply for the result of `cf_phone_exchange` or `cf_phone_pair_code`; [line] is read only for 0. */
internal inline fun phoneReply(
    code: Int,
    line: () -> String?,
): PhoneReply =
    when (code) {
        NATIVE_OK -> PhoneReply(PhoneReplyKind.Line, line())
        NATIVE_REPLY_CLOSED -> PhoneReply(PhoneReplyKind.Closed)
        NATIVE_CODE_REFUSED -> PhoneReply(PhoneReplyKind.Refused)
        else -> PhoneReply(PhoneReplyKind.Unavailable)
    }
