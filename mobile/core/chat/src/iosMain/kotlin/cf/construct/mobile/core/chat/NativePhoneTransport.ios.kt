@file:OptIn(ExperimentalForeignApi::class)

package cf.construct.mobile.core.chat

import cf.construct.mobile.core.chat.bridge.cf_phone_close
import cf.construct.mobile.core.chat.bridge.cf_phone_exchange
import cf.construct.mobile.core.chat.bridge.cf_phone_fingerprint
import cf.construct.mobile.core.chat.bridge.cf_phone_is_numeric_address
import cf.construct.mobile.core.chat.bridge.cf_phone_open
import cf.construct.mobile.core.chat.bridge.cf_phone_pair_code
import cf.construct.mobile.core.chat.bridge.cf_phone_proof
import kotlinx.cinterop.ByteVar
import kotlinx.cinterop.COpaquePointer
import kotlinx.cinterop.COpaquePointerVar
import kotlinx.cinterop.CPointer
import kotlinx.cinterop.CPointerVar
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.alloc
import kotlinx.cinterop.memScoped
import kotlinx.cinterop.ptr
import kotlinx.cinterop.value

internal actual object NativePhoneTransport : PhoneTransport {
    override fun isNumericAddress(host: String): Boolean = cf_phone_is_numeric_address(host).toInt() != 0

    override fun open(
        host: String,
        port: Int,
        pin: String?,
    ): PhoneChannel? =
        memScoped {
            val out = alloc<COpaquePointerVar>()
            when (cf_phone_open(host, port.toUShort(), pin, out.ptr)) {
                0 -> IosPhoneChannel(checkNotNull(out.value))
                2 -> throw PhoneIdentityMismatch()
                NATIVE_COMPUTER_UPDATE -> throw PhoneProtocolMismatch()
                else -> null
            }
        }

    override fun proof(
        keyHex: String,
        fingerprint: String,
        agent: Boolean,
    ): String? = take(cf_phone_proof(keyHex, fingerprint, if (agent) 1u else 0u))
}

private class IosPhoneChannel(
    private val handle: COpaquePointer,
) : PhoneChannel {
    override val fingerprint = take(cf_phone_fingerprint(handle)).orEmpty()

    override fun exchange(line: String): PhoneReply = reply { cf_phone_exchange(handle, line, it) }

    override fun pairCode(
        code: String,
        version: Int,
        name: String,
    ): PhoneReply = reply { cf_phone_pair_code(handle, code, version.toULong(), name, it) }

    private fun reply(call: (CPointer<CPointerVar<ByteVar>>) -> Int): PhoneReply =
        memScoped {
            val out = alloc<CPointerVar<ByteVar>>()
            when (call(out.ptr)) {
                0 -> PhoneReply(PhoneReplyKind.Line, take(out.value))
                2 -> PhoneReply(PhoneReplyKind.Closed)
                NATIVE_CODE_REFUSED -> PhoneReply(PhoneReplyKind.Refused)
                else -> PhoneReply(PhoneReplyKind.Unavailable)
            }
        }

    override fun close() = cf_phone_close(handle)
}
