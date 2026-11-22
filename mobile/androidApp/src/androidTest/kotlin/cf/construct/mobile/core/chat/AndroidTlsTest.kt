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
 * system must refuse it: the client then ends the handshake with the TLS alert `unknown_ca`. A core that was not
 * given the JVM fails otherwise (without an alert), which is what this test tells apart.
 */
class AndroidTlsTest {
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
            assertEquals(
                ModelTestResult.Failed("unreachable: nothing answered at this address."),
                result,
            )
            val message = assertNotNull(failure, "the client did not start a handshake").message.orEmpty()
            assertTrue(
                message.contains("UNKNOWN_CA"),
                "the client did not refuse the certificate: $message",
            )
        } finally {
            server.close()
        }
    }
}
