package cf.construct.mobile.core.sync

import cf.construct.mobile.core.chat.PhoneChannel
import cf.construct.mobile.core.chat.PhoneIdentityMismatch
import cf.construct.mobile.core.chat.PhoneReply
import cf.construct.mobile.core.chat.PhoneReplyKind
import cf.construct.mobile.core.chat.PhoneTransport
import java.io.BufferedReader
import java.io.IOException
import java.io.InputStreamReader
import java.net.InetSocketAddress
import java.security.MessageDigest
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLHandshakeException
import javax.net.ssl.SSLSocket
import javax.net.ssl.X509TrustManager

/**
 * The phone's transport written for the JVM, so a test can speak to the agent's own test program as a phone does:
 * TLS 1.3 to a certificate pinned by its SHA-256 fingerprint, and one JSON line per call. The phone app uses the
 * Rust one; this exists only so the sync can be tested against the real agent where no phone runs.
 */
class JvmPhoneTransport : PhoneTransport {
    override fun isNumericAddress(host: String) = host.all { it.isDigit() || it == '.' } || ':' in host

    override fun open(
        host: String,
        port: Int,
        pin: String?,
    ): PhoneChannel? = pin?.let { connect(host, port, it) }

    private fun connect(
        host: String,
        port: Int,
        pin: String,
    ): PhoneChannel? {
        val context = SSLContext.getInstance("TLSv1.3")
        context.init(null, arrayOf(Pinned(pin)), null)
        val socket = context.socketFactory.createSocket() as SSLSocket
        return try {
            socket.connect(InetSocketAddress(host, port), CONNECT_MILLIS)
            socket.soTimeout = READ_MILLIS
            socket.startHandshake()
            Channel(socket, pin)
        } catch (_: SSLHandshakeException) {
            socket.close()
            throw PhoneIdentityMismatch()
        } catch (_: IOException) {
            socket.close()
            null
        }
    }

    override fun proof(
        keyHex: String,
        fingerprint: String,
        agent: Boolean,
    ): String {
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(keyHex.hexBytes(), "HmacSHA256"))
        val role = if (agent) "agent" else "phone"
        return mac.doFinal("construct-pair\u0000$role".toByteArray() + fingerprint.hexBytes()).hex()
    }

    private class Pinned(
        private val pin: String,
    ) : X509TrustManager {
        override fun checkClientTrusted(
            chain: Array<X509Certificate>,
            authType: String,
        ) = throw CertificateException()

        override fun checkServerTrusted(
            chain: Array<X509Certificate>,
            authType: String,
        ) {
            if (MessageDigest.getInstance("SHA-256").digest(chain.first().encoded).hex() !=
                pin
            ) {
                throw CertificateException()
            }
        }

        override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
    }

    private class Channel(
        private val socket: SSLSocket,
        override val fingerprint: String,
    ) : PhoneChannel {
        private val reader = BufferedReader(InputStreamReader(socket.inputStream, Charsets.UTF_8))

        override fun exchange(line: String): PhoneReply =
            try {
                socket.outputStream.write((line + "\n").toByteArray(Charsets.UTF_8))
                socket.outputStream.flush()
                reader.readLine()?.let { PhoneReply(PhoneReplyKind.Line, it) } ?: PhoneReply(PhoneReplyKind.Closed)
            } catch (_: IOException) {
                PhoneReply(PhoneReplyKind.Unavailable)
            }

        override fun pairCode(
            code: String,
            version: Int,
            name: String,
        ): PhoneReply = error("The typed code is not used by this transport.")

        override fun close() = socket.close()
    }

    private companion object {
        const val CONNECT_MILLIS = 5_000
        const val READ_MILLIS = 20_000

        fun ByteArray.hex() = joinToString("") { "%02x".format(it) }

        fun String.hexBytes() = chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    }
}
