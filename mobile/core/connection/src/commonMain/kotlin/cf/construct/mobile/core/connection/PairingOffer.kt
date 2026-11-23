package cf.construct.mobile.core.connection

private const val DEFAULT_PORT = 47821
private const val CODE_LENGTH = 8
private const val HEX_LENGTH = 64

internal data class ComputerAddress(
    val host: String,
    val port: Int,
) {
    companion object {
        fun parse(
            input: String,
            isNumericAddress: (String) -> Boolean,
        ): ComputerAddress? {
            val text = input.trim()
            if (text.length > MAX_ADDRESS_LENGTH || '\u0000' in text) return null
            val endpoint = if (text.startsWith('[')) parseBracketed(text) else parseHost(text)
            return endpoint?.takeIf { isNumericAddress(it.host) }
        }

        private fun parseHost(text: String): ComputerAddress? {
            val (host, port) = text.split(':', limit = 2).let { it.first() to it.getOrNull(1) }
            return withPort(host, port)
        }

        private fun parseBracketed(text: String): ComputerAddress? {
            val host = text.substringAfter('[').substringBefore(']')
            val suffix = text.substringAfter(']', missingDelimiterValue = "?")
            val validSuffix = suffix.isEmpty() || suffix.startsWith(':')
            if (!validSuffix || ':' !in host) return null
            return withPort(host, suffix.takeIf { it.isNotEmpty() }?.drop(1))
        }

        private fun withPort(
            host: String,
            text: String?,
        ): ComputerAddress? {
            val port = if (text == null) DEFAULT_PORT else text.toIntOrNull() ?: return null
            return if (port in 1..UShort.MAX_VALUE.toInt()) ComputerAddress(host, port) else null
        }

        private const val MAX_ADDRESS_LENGTH = 255
    }
}

internal fun String.isFingerprint() = length == HEX_LENGTH && all { it in '0'..'9' || it in 'a'..'f' }

/** The secret stays in this object until the proof exchange. Its description carries no fields. */
internal class PairingOffer(
    val address: ComputerAddress,
    val secret: String,
    val pin: String?,
) {
    override fun toString() = "Pairing offer"

    companion object {
        fun code(
            address: ComputerAddress,
            code: String,
        ): PairingOffer? {
            val digits = code.trim()
            if (digits.length != CODE_LENGTH || digits.any { it !in '0'..'9' }) return null
            return PairingOffer(address, digits, null)
        }

        fun qr(
            payload: String,
            isNumericAddress: (String) -> Boolean,
        ): PairingOffer? {
            val text = payload.trim()
            val scheme = "construct-pair://"
            val address =
                text
                    .takeIf { it.startsWith(scheme) }
                    ?.removePrefix(scheme)
                    ?.substringBefore('?')
                    ?.let { ComputerAddress.parse(it, isNumericAddress) } ?: return null
            val query = parseQuery(text.substringAfter('?', missingDelimiterValue = ""))
            return query?.let { qrOffer(address, it) }
        }

        private fun parseQuery(text: String): Map<String, String>? {
            val fields =
                text
                    .split('&')
                    .map { it.substringBefore('=') to it.substringAfter('=', missingDelimiterValue = "") }
            val query = fields.toMap()
            return query.takeIf { fields.size == it.size && it.keys == setOf("v", "fp", "s") }
        }

        private fun qrOffer(
            address: ComputerAddress,
            query: Map<String, String>,
        ): PairingOffer? {
            query.getValue("v").toUIntOrNull() ?: return null
            val pin = query.getValue("fp").lowercase()
            val secret = query.getValue("s").lowercase()
            return if (pin.isFingerprint() && secret.isFingerprint()) PairingOffer(address, secret, pin) else null
        }
    }
}
