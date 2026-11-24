package cf.construct.mobile.core.chat

import com.sun.jna.Pointer
import com.sun.jna.ptr.PointerByReference

internal actual object NativePhoneTransport : PhoneTransport {
    override fun isNumericAddress(host: String): Boolean =
        bridge.cf_phone_is_numeric_address(cString(host)).toInt() != 0

    override fun open(
        host: String,
        port: Int,
        pin: String?,
    ): PhoneChannel? {
        val out = PointerByReference()
        val code = bridge.cf_phone_open(cString(host), port.toShort(), pin?.let(::cString), out)
        return if (code == NATIVE_OK) AndroidPhoneChannel(checkNotNull(out.value)) else openFailure(code)
    }

    override fun proof(
        keyHex: String,
        fingerprint: String,
        agent: Boolean,
    ): String? = take(bridge.cf_phone_proof(cString(keyHex), cString(fingerprint), if (agent) 1 else 0))
}

private class AndroidPhoneChannel(
    private val handle: Pointer,
) : PhoneChannel {
    override val fingerprint = take(bridge.cf_phone_fingerprint(handle)).orEmpty()

    override fun exchange(line: String): PhoneReply = reply { bridge.cf_phone_exchange(handle, cString(line), it) }

    override fun pairCode(
        code: String,
        version: Int,
        name: String,
    ): PhoneReply = reply { bridge.cf_phone_pair_code(handle, cString(code), version.toLong(), cString(name), it) }

    private fun reply(call: (PointerByReference) -> Int): PhoneReply {
        val out = PointerByReference()
        return phoneReply(call(out)) { take(out.value) }
    }

    override fun close() = bridge.cf_phone_close(handle)
}
