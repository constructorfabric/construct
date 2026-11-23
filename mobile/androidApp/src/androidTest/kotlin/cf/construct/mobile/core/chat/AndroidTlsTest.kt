package cf.construct.mobile.core.chat

import androidx.test.platform.app.InstrumentationRegistry
import java.net.InetAddress
import java.security.KeyStore
import javax.net.ssl.KeyManagerFactory
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLSocket
import kotlin.concurrent.thread
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * An `https` address on Android: the chat core asks the system to check the certificate, through the
 * JVM that [initChatCore] hands over. The server here has a certificate no phone trusts, so the
 * system must refuse it: the client then ends the handshake with the TLS alert `unknown_ca`. A core
 * that was not given the JVM fails otherwise (without an alert), which is what this test tells apart.
 *
 * The first `https` call of a process can spend ten seconds or more in the system's check on an
 * emulator (measured: 10 s, and once past the core's 15 s deadline, so `timeout`); the next ones take
 * well under a second. The test makes one call first, to take that time out of the one it asserts on.
 */
class AndroidTlsTest {
    /** What one call to a TLS server with an untrusted certificate came to, on both ends. */
    private class Attempt(
        val result: ModelTestResult,
        val serverSaw: Exception?,
    )

    private fun attempt(tls: SSLContext): Attempt {
        val server = tls.serverSocketFactory.createServerSocket(0, 1, InetAddress.getByName("127.0.0.1"))
        var failure: Exception? = null
        val acceptor =
            thread(isDaemon = true) {
                val socket = server.accept() as SSLSocket
                failure = runCatching { socket.startHandshake() }.exceptionOrNull() as? Exception
                socket.close()
            }
        try {
            val result =
                nativeChatCore().testModel(
                    "https://127.0.0.1:${server.localPort}",
                    "model",
                    "sk-fake-key-4f9a2c71",
                )
            acceptor.join(10_000)
            return Attempt(result, failure)
        } finally {
            server.close()
        }
    }

    @Test
    fun aCertificateNoPhoneTrustsIsRefusedByTheSystemCheck() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        // The test runs inside the app, whose `Application.onCreate` has run: nothing here starts the
        // core, so the test fails if that call is gone.
        assertTrue(chatCoreIsReady(), "ConstructApp did not start the chat core")

        val keys = KeyStore.getInstance("PKCS12")
        instrumentation.context.assets
            .open("untrusted-server.p12")
            .use { keys.load(it, "untrusted".toCharArray()) }
        val factory =
            KeyManagerFactory.getInstance(KeyManagerFactory.getDefaultAlgorithm()).apply {
                init(keys, "untrusted".toCharArray())
            }
        val tls = SSLContext.getInstance("TLS").apply { init(factory.keyManagers, null, null) }

        attempt(tls) // takes the first call's time out of the next one
        val second = attempt(tls)
        assertEquals(
            ModelTestResult.Failed("unreachable: nothing answered at this address."),
            second.result,
        )
        val message = assertNotNull(second.serverSaw, "the client did not start a handshake").message.orEmpty()
        assertTrue(
            message.contains("UNKNOWN_CA"),
            "the client did not refuse the certificate: $message",
        )
    }
}
