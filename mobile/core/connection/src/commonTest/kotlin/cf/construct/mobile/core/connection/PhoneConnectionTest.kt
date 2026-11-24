package cf.construct.mobile.core.connection

import cf.construct.mobile.core.chat.PhoneChannel
import cf.construct.mobile.core.chat.PhoneIdentityMismatch
import cf.construct.mobile.core.chat.PhoneProtocolMismatch
import cf.construct.mobile.core.chat.PhoneReply
import cf.construct.mobile.core.chat.PhoneReplyKind
import cf.construct.mobile.core.chat.PhoneTransport
import cf.construct.mobile.core.keys.fakePairingSecret
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue
import kotlin.time.Duration.Companion.seconds

@OptIn(ExperimentalCoroutinesApi::class)
class PhoneConnectionTest {
    @Test
    fun typedPairingUsesTheAuthenticatedTransportAndPinsTheComputerAcrossAClientRestart() =
        runTest {
            val fixture = Fixture(this)
            assertEquals(PairResult.Paired, fixture.connection().pairByCode(ADDRESS, CODE, "Phone"))
            assertEquals(1, fixture.agent.codeExchanges)
            assertTrue(fixture.agent.calls.isEmpty())
            assertNotNull(fixture.store.read())

            val observed = fixture.watch()
            assertEquals(ComputerLink.Connected, observed.last().link)
            assertTrue(fixture.agent.lastPin == PIN)
            assertTrue(
                fixture.agent.calls
                    .last()
                    .contains("ping"),
            )
            assertFalse(observed.toString().contains(TOKEN))
            assertFalse(observed.toString().contains(PIN))
        }

    @Test
    fun qrPinIsCheckedBeforeSendingAProofAndAnInvalidAgentProofIsNeverSaved() =
        runTest {
            val fixture = Fixture(this)
            val qr = "construct-pair://$ADDRESS?v=1&fp=$PIN&s=$SECRET"
            fixture.agent.fingerprint = OTHER_PIN
            assertEquals(PairResult.WrongComputer, fixture.connection().pairByQr(qr, "Phone"))
            assertTrue(fixture.agent.calls.isEmpty())

            fixture.agent.fingerprint = PIN
            fixture.agent.agentProof = OTHER_PIN
            assertEquals(PairResult.WrongComputer, fixture.connection().pairByQr(qr, "Phone"))
            assertFalse(
                fixture.agent.calls
                    .single()
                    .contains(SECRET),
            )
            assertTrue(fixture.store.read() == null)

            fixture.agent.agentProof = AGENT_PROOF
            assertEquals(PairResult.Paired, fixture.connection().pairByQr(qr, "Phone"))
        }

    @Test
    fun onlyAnAuthenticatedVersionReplyCanAskAOneTimeCodeClientToUpdate() =
        runTest {
            val fixture = Fixture(this)
            fixture.agent.update = true
            fixture.agent.codeFailure = PhoneReplyKind.Refused
            val old = fixture.connection(version = 0)
            assertEquals(PairResult.FreshCode, old.pairByCode(ADDRESS, CODE, "Phone"))
            fixture.agent.codeFailure = null
            assertEquals(PairResult.PhoneUpdate, old.pairByCode(ADDRESS, CODE, "Phone"))
            assertTrue(fixture.store.read() == null)
        }

    @Test
    fun aComputerThatRejectsTheTypedProtocolGetsAnUpdateMessage() =
        runTest {
            val fixture = Fixture(this)
            fixture.agent.protocolMismatch = true
            val result = fixture.connection().pairByCode(ADDRESS, CODE, "Phone")
            assertEquals("Update Construct on the computer to connect.", result.message)
            assertTrue(fixture.store.read() == null)
        }

    @Test
    fun aSilentTypedPeerLeavesThePhoneFreeToRetry() =
        runTest {
            val fixture = Fixture(this)
            val phone = fixture.connection()
            for (failure in listOf(PhoneReplyKind.Unavailable, PhoneReplyKind.Closed)) {
                fixture.agent.codeFailure = failure
                assertEquals(PairResult.Unreachable, phone.pairByCode(ADDRESS, CODE, "Phone"))
                assertTrue(fixture.store.read() == null)
            }
            fixture.agent.codeFailure = null
            assertEquals(PairResult.Paired, phone.pairByCode(ADDRESS, CODE, "Phone"))
        }

    @Test
    fun aLocalNetworkDenialExplainsFailedPairingAndFailedReconnect() =
        runTest {
            val fixture = Fixture(this)
            val phone = fixture.connection()
            fixture.agent.codeFailure = PhoneReplyKind.Unavailable
            fixture.localNetworkDenied = true
            assertEquals(PairResult.NetworkPermission, phone.pairByCode(ADDRESS, CODE, "Phone"))
            assertTrue(fixture.store.read() == null)

            fixture.agent.codeFailure = null
            fixture.localNetworkDenied = false
            assertEquals(PairResult.Paired, phone.pairByCode(ADDRESS, CODE, "Phone"))
            val observed = fixture.watch()
            assertEquals(ComputerLink.Connected, observed.last().link)

            fixture.agent.action = PhoneReplyKind.Closed
            fixture.localNetworkDenied = true
            tick()
            assertEquals(ComputerLink.NetworkPermission, observed.last().link)
            assertNotNull(fixture.store.read())
        }

    @Test
    fun aSavedPairingSurvivesALostReplyAndARejectedCodeUntilReplacementIsConfirmed() =
        runTest {
            val fixture = Fixture(this)
            val phone = fixture.connection()
            assertEquals(PairResult.Paired, phone.pairByCode(ADDRESS, CODE, "Phone"))
            val saved = fixture.store.read()

            fixture.agent.codeFailure = PhoneReplyKind.Unavailable
            assertEquals(PairResult.Unreachable, phone.pairByCode(ADDRESS, CODE, "Phone"))
            assertTrue(fixture.store.read() == saved)
            fixture.agent.codeFailure = PhoneReplyKind.Refused
            assertEquals(PairResult.FreshCode, phone.pairByCode(ADDRESS, CODE, "Phone"))
            assertTrue(fixture.store.read() == saved)

            fixture.agent.codeFailure = null
            fixture.agent.token = "12".repeat(32)
            assertEquals(PairResult.Paired, phone.pairByCode(ADDRESS, "98765432", "Phone"))
            assertTrue(fixture.store.loadPairing(fixture.agent::isNumericAddress)?.token == fixture.agent.token)
        }

    @Test
    fun lostNetworkAndSilentComputerKeepThePairingAndAuthenticatedRevocationRemovesIt() =
        runTest {
            val fixture = Fixture(this)
            assertEquals(PairResult.Paired, fixture.connection().pairByCode(ADDRESS, CODE, "Phone"))
            val observed = fixture.watch()
            assertEquals(LinkSnapshot(ComputerLink.Connected, 1000, ADDRESS), observed.last())

            fixture.agent.action = PhoneReplyKind.Unavailable
            fixture.network = false
            tick()
            assertEquals(LinkSnapshot(ComputerLink.NoNetwork, 1000, ADDRESS), observed.last())
            assertNotNull(fixture.store.read())

            fixture.network = true
            tick()
            assertEquals(LinkSnapshot(ComputerLink.Unreachable, 1000, ADDRESS), observed.last())
            assertNotNull(fixture.store.read())

            fixture.agent.action = PhoneReplyKind.Line
            tick()
            assertEquals(LinkSnapshot(ComputerLink.Connected, 1000, ADDRESS), observed.last())

            fixture.agent.revoked = true
            tick()
            assertEquals(LinkSnapshot(ComputerLink.NotPaired, 0), observed.last())
            assertTrue(fixture.store.read() == null)
        }

    @Test
    fun aStoppedMonitorSavesItsNewestContactAndAnOfflineRevocationDoesNotClearIt() =
        runTest {
            val fixture = Fixture(this)
            val connection = fixture.connection()
            assertEquals(PairResult.Paired, connection.pairByCode(ADDRESS, CODE, "Phone"))

            val active = mutableListOf<LinkSnapshot>()
            val watch = backgroundScope.launch { connection.monitor { active.add(it) } }
            runCurrent()
            fixture.now = 2000
            tick()
            assertEquals(LinkSnapshot(ComputerLink.Connected, 2000, ADDRESS), active.last())
            assertEquals(1000L, fixture.store.loadPairing(fixture.agent::isNumericAddress)?.lastContactMillis)
            watch.cancelAndJoin()
            assertEquals(2000L, fixture.store.loadPairing(fixture.agent::isNumericAddress)?.lastContactMillis)

            fixture.agent.action = PhoneReplyKind.Closed
            val restarted = fixture.watch()
            assertEquals(LinkSnapshot(ComputerLink.Unreachable, 2000, ADDRESS), restarted.last())
            assertNotNull(fixture.store.loadPairing(fixture.agent::isNumericAddress))
        }

    @Test
    fun localUnpairRemovesTheCopyAndDoesNotRestoreTheSavedPairing() =
        runTest {
            val fixture = Fixture(this)
            val connection = fixture.connection()
            assertEquals(PairResult.Paired, connection.pairByCode(ADDRESS, CODE, "Phone"))
            val observed = mutableListOf<LinkSnapshot>()
            backgroundScope.launch { connection.monitor { observed.add(it) } }
            runCurrent()
            fixture.now = 2000
            tick()

            var removed = false
            connection.unpair { removed = true }
            runCurrent()
            assertTrue(removed)
            assertEquals(LinkSnapshot(ComputerLink.NotPaired, 0), observed.last())
            assertTrue(fixture.store.read() == null)
        }

    @Test
    fun invalidInputDoesNotOpenAChannelOrEchoCredentials() =
        runTest {
            val fixture = Fixture(this)
            val connection = fixture.connection()
            assertEquals(PairResult.InvalidAddress, connection.pairByCode("http://$ADDRESS/$CODE", CODE, "Phone"))
            assertEquals(PairResult.InvalidAddress, connection.pairByCode("[2001:::1]", CODE, "Phone"))
            assertEquals(PairResult.InvalidAddress, connection.pairByCode("computer.local", CODE, "Phone"))
            assertEquals(
                ComputerAddress("2001:db8::1", 4242),
                ComputerAddress.parse("[2001:db8::1]:4242", fixture.agent::isNumericAddress),
            )
            assertNotNull(ComputerAddress.parse("[::ffff:192.0.2.10]", fixture.agent::isNumericAddress))
            assertEquals(PairResult.InvalidCode, connection.pairByCode(ADDRESS, SECRET, "Phone"))
            assertEquals(PairResult.InvalidQr, connection.pairByQr("construct-pair://$ADDRESS?s=$SECRET", "Phone"))
            assertTrue(fixture.agent.calls.isEmpty())
            assertFalse(PhoneReply(PhoneReplyKind.Line, TOKEN).toString().contains(TOKEN))
        }

    private fun TestScope.tick() {
        advanceTimeBy(1.seconds)
        runCurrent()
    }

    private class Fixture(
        private val scope: TestScope,
    ) {
        val store = fakePairingSecret()
        val agent = FakeAgent()
        var network = true
        var localNetworkDenied = false
        var now = 1000L

        fun connection(version: Int = PHONE_PROTOCOL) =
            PhoneConnection(
                store,
                agent,
                { network },
                version,
                StandardTestDispatcher(scope.testScheduler),
                1.seconds,
                { now },
                localNetworkDenied = { address ->
                    assertEquals("192.0.2.10", address.host)
                    assertEquals(47821, address.port)
                    localNetworkDenied
                },
            )

        fun watch(): List<LinkSnapshot> {
            val observed = mutableListOf<LinkSnapshot>()
            scope.backgroundScope.launch { connection().monitor { observed.add(it) } }
            scope.runCurrent()
            return observed
        }
    }

    private class FakeAgent : PhoneTransport {
        var fingerprint = PIN
        var lastPin: String? = null
        var update = false
        var agentProof = AGENT_PROOF
        var codeFailure: PhoneReplyKind? = null
        var token = TOKEN
        var protocolMismatch = false
        var codeExchanges = 0
        var revoked = false
        var action = PhoneReplyKind.Line
        val calls = mutableListOf<String>()

        override fun isNumericAddress(host: String): Boolean =
            host in setOf("192.0.2.10", "2001:db8::1", "::ffff:192.0.2.10")

        override fun open(
            host: String,
            port: Int,
            pin: String?,
        ): PhoneChannel? {
            lastPin = pin
            if (pin == null && protocolMismatch) throw PhoneProtocolMismatch()
            if (pin != null && pin != fingerprint) throw PhoneIdentityMismatch()
            assertEquals("192.0.2.10", host)
            assertEquals(47821, port)
            return object : PhoneChannel {
                override val fingerprint = this@FakeAgent.fingerprint

                override fun exchange(line: String): PhoneReply {
                    calls.add(line)
                    return when {
                        line.contains("\"pair\"") -> pair()
                        action != PhoneReplyKind.Line -> PhoneReply(action)
                        revoked && line.contains("\"ping\"") -> PhoneReply(PhoneReplyKind.Line, """{"revoked":true}""")
                        else -> PhoneReply(PhoneReplyKind.Line, """{"v":1,"ok":true}""")
                    }
                }

                override fun pairCode(
                    code: String,
                    version: Int,
                    name: String,
                ): PhoneReply {
                    codeExchanges++
                    return codeFailure?.let { PhoneReply(it) } ?: pair()
                }

                override fun close() = Unit
            }
        }

        override fun proof(
            keyHex: String,
            fingerprint: String,
            agent: Boolean,
        ): String = if (agent) AGENT_PROOF else PHONE_PROOF

        private fun pair(): PhoneReply {
            val line =
                if (update) {
                    """{"v":1,"error":"update needed","min_version":1,"agent_proof":"$agentProof"}"""
                } else {
                    """{"v":1,"paired":true,"token":"$token","agent_proof":"$agentProof"}"""
                }
            return PhoneReply(PhoneReplyKind.Line, line)
        }
    }

    private companion object {
        const val ADDRESS = "192.0.2.10:47821"
        const val CODE = "01234567"
        val PIN = "a1".repeat(32)
        val OTHER_PIN = "b2".repeat(32)
        val TOKEN = "e3".repeat(32)
        val SECRET = "f4".repeat(32)
        val AGENT_PROOF = "c5".repeat(32)
        val PHONE_PROOF = "d6".repeat(32)
    }
}
