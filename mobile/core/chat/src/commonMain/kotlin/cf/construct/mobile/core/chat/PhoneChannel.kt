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

    /** A line, a deliberate TLS close with no line, or loss of the connection. */
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
class PhoneReply(
    val kind: PhoneReplyKind,
    val line: String? = null,
) {
    override fun toString() = kind.name
}

fun nativePhoneTransport(): PhoneTransport = NativePhoneTransport

internal expect object NativePhoneTransport : PhoneTransport

// Status codes from construct_chat.h.
internal const val NATIVE_CODE_REFUSED = 3
internal const val NATIVE_COMPUTER_UPDATE = 4
