package cf.construct.mobile.core.connection

import cf.construct.mobile.core.chat.PhoneChannel
import cf.construct.mobile.core.chat.PhoneIdentityMismatch
import cf.construct.mobile.core.chat.PhoneProtocolMismatch
import cf.construct.mobile.core.chat.PhoneReply
import cf.construct.mobile.core.chat.PhoneReplyKind
import cf.construct.mobile.core.chat.PhoneTransport
import cf.construct.mobile.core.keys.ModelSettingsException
import cf.construct.mobile.core.keys.fakePairingSecret
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
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
            assertEquals(PairResult.TypedCodeUnsupported, result)
            assertEquals(
                "This address does not accept a typed code. Update Construct on the computer or use its QR code.",
                result.message,
            )
            assertTrue(fixture.store.read() == null)
        }

    @Test
    fun anUpdateIsToldOnBothRoutesAndWhileWatching() =
        runTest {
            val fixture = Fixture(this)
            val qr = "construct-pair://$ADDRESS?v=1&fp=$PIN&s=$SECRET"
            val phoneUpdate = "Update this app to connect to the computer."
            val computerUpdate = "Update Construct on the computer to connect."

            fixture.agent.update = true
            val old = fixture.connection(version = 0)
            assertEquals(phoneUpdate, old.pairByQr(qr, "Phone").message)
            assertEquals(phoneUpdate, old.pairByCode(ADDRESS, CODE, "Phone").message)

            fixture.agent.update = false
            fixture.agent.agentUpdate = true
            val newer = fixture.connection(version = 2)
            assertEquals(computerUpdate, newer.pairByQr(qr, "Phone").message)
            assertEquals(computerUpdate, newer.pairByCode(ADDRESS, CODE, "Phone").message)
            assertTrue(fixture.store.read() == null)

            fixture.agent.agentUpdate = false
            assertEquals(PairResult.Paired, fixture.connection().pairByCode(ADDRESS, CODE, "Phone"))
            fixture.agent.authReply = """{"v":1,"error":"update needed","min_version":1}"""
            assertEquals(ComputerLink.PhoneUpdate, fixture.watch(version = 0).last().link)
            fixture.agent.authReply = """{"v":1,"error":"agent update needed"}"""
            assertEquals(ComputerLink.ComputerUpdate, fixture.watch(version = 2).last().link)
        }

    @Test
    fun thePhoneSendsTheLinesTheComputerExpects() =
        runTest {
            val fixture = Fixture(this)
            val longName = "N".repeat(100)
            val qr = "construct-pair://$ADDRESS?v=1&fp=$PIN&s=$SECRET"
            assertEquals(PairResult.Paired, fixture.connection().pairByQr(qr, longName))
            val hello = Json.parseToJsonElement(fixture.agent.calls.single()).jsonObject
            assertEquals(setOf("v", "pair"), hello.keys)
            assertEquals(1, hello.getValue("v").jsonPrimitive.int)
            val pair = hello.getValue("pair").jsonObject
            assertEquals(setOf("proof", "name"), pair.keys)
            assertEquals(PHONE_PROOF, pair.getValue("proof").jsonPrimitive.content)
            assertEquals("N".repeat(64), pair.getValue("name").jsonPrimitive.content)

            val typed = Fixture(this)
            assertEquals(PairResult.Paired, typed.connection().pairByCode(ADDRESS, CODE, longName))
            assertEquals(Triple(CODE, 1, "N".repeat(64)), typed.agent.codeRequest)

            typed.watch()
            val lines = typed.agent.calls.map { Json.parseToJsonElement(it).jsonObject }
            assertEquals(setOf("v", "token", "watch_revocation"), lines[0].keys)
            assertEquals(TOKEN, lines[0].getValue("token").jsonPrimitive.content)
            assertEquals(JsonPrimitive(true), lines[0].getValue("watch_revocation"))
            assertEquals(setOf("cmd"), lines[1].keys)
            assertEquals(JsonPrimitive("ping"), lines[1].getValue("cmd"))
        }

    @Test
    fun noSecretAppearsInAnyTextThePhoneCanPublish() =
        runTest {
            val fixture = Fixture(this)
            val address = ComputerAddress("192.0.2.10", 47821)
            val published = mutableListOf<String>()
            published += PairingOffer.code(address, CODE).toString()
            published +=
                PairingOffer
                    .qr(
                        "construct-pair://$ADDRESS?v=1&fp=$PIN&s=$SECRET",
                        fixture.agent::isNumericAddress,
                    ).toString()
            published += StoredPairing(address, PIN, TOKEN, 1000).toString()
            published +=
                PhoneReply(PhoneReplyKind.Line, """{"token":"$TOKEN","agent_proof":"$AGENT_PROOF"}""").toString()
            published += PairResult.entries.flatMap { listOf(it.toString(), it.message.orEmpty()) }
            published += PhoneIdentityMismatch().message.orEmpty()
            published += PhoneProtocolMismatch().message.orEmpty()

            // Every way a pairing can fail with the secrets in play.
            val qr = "construct-pair://$ADDRESS?v=1&fp=$PIN&s=$SECRET"
            fixture.agent.agentProof = OTHER_PIN
            published += fixture.connection().pairByQr(qr, "Phone").let { "${it.message} $it" }
            fixture.agent.agentProof = AGENT_PROOF
            fixture.agent.fingerprint = OTHER_PIN
            published += fixture.connection().pairByQr(qr, "Phone").let { "${it.message} $it" }
            fixture.agent.fingerprint = PIN
            for (failure in listOf(PhoneReplyKind.Unavailable, PhoneReplyKind.Closed, PhoneReplyKind.Refused)) {
                fixture.agent.codeFailure = failure
                published += fixture.connection().pairByCode(ADDRESS, CODE, "Phone").let { "${it.message} $it" }
            }
            fixture.agent.codeFailure = null

            // A watched pairing, an unreadable record and the failure it raises.
            assertEquals(PairResult.Paired, fixture.connection().pairByCode(ADDRESS, CODE, "Phone"))
            published += fixture.watch().map { it.toString() }
            fixture.store.save("record $TOKEN $PIN")
            published += fixture.watch().map { it.toString() }
            val failure =
                assertFailsWith<ModelSettingsException> { fixture.store.loadPairing(fixture.agent::isNumericAddress) }
            published += listOf(failure.message.orEmpty(), failure.toString(), failure.cause?.toString().orEmpty())

            assertTrue(published.size > 20)
            for (text in published) {
                for (secret in listOf(CODE, SECRET, PIN, OTHER_PIN, TOKEN, AGENT_PROOF, PHONE_PROOF)) {
                    assertFalse(text.contains(secret), "a secret appears in: $text")
                }
            }
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
    fun aPairingThatCannotBeRemovedThrowsAfterTheCopyIsGoneAndUnpairsOnTheNextTry() =
        runTest {
            val fixture = Fixture(this)
            val connection = fixture.connection()
            assertEquals(PairResult.Paired, connection.pairByCode(ADDRESS, CODE, "Phone"))
            val observed = mutableListOf<LinkSnapshot>()
            backgroundScope.launch { connection.monitor { observed.add(it) } }
            runCurrent()

            fixture.storeFailing = true
            var removed = false
            assertFailsWith<ModelSettingsException> { connection.unpair { removed = true } }
            assertTrue(removed)
            fixture.storeFailing = false
            assertNotNull(fixture.store.read())

            connection.unpair {}
            runCurrent()
            assertTrue(fixture.store.read() == null)
            assertEquals(LinkSnapshot(ComputerLink.NotPaired, 0), observed.last())
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
        }

    @Test
    fun aRecordKeepsAnIpv6AddressAndAnUnreadableRecordIsShownUntilThePersonPairsAgain() =
        runTest {
            val fixture = Fixture(this)
            val saved = StoredPairing(ComputerAddress("2001:db8::1", 4242), PIN, TOKEN, 5)
            fixture.store.savePairing(saved)
            assertEquals(saved, fixture.store.loadPairing(fixture.agent::isNumericAddress))

            fixture.store.save("not a record")
            val connection = fixture.connection()
            val observed = mutableListOf<LinkSnapshot>()
            backgroundScope.launch { connection.monitor { observed.add(it) } }
            runCurrent()
            assertEquals(LinkSnapshot(ComputerLink.StorageError, 0), observed.last())
            tick()
            assertEquals(LinkSnapshot(ComputerLink.StorageError, 0), observed.last())
            assertEquals("not a record", fixture.store.read())

            assertEquals(PairResult.Paired, connection.pairByCode(ADDRESS, CODE, "Phone"))
            runCurrent()
            assertEquals(ComputerLink.Connected, observed.last().link)
        }

    @Test
    fun aFailedWriteOfTheLastContactDoesNotStopTheChecks() =
        runTest {
            val fixture = Fixture(this)
            assertEquals(PairResult.Paired, fixture.connection().pairByCode(ADDRESS, CODE, "Phone"))
            val observed = fixture.watch()

            fixture.storeFailing = true
            fixture.now = 40_000
            tick()
            assertEquals(LinkSnapshot(ComputerLink.Connected, 40_000, ADDRESS), observed.last())
            fixture.now = 41_000
            tick()
            assertEquals(LinkSnapshot(ComputerLink.Connected, 41_000, ADDRESS), observed.last())
            assertEquals(1000L, fixture.store.loadPairing(fixture.agent::isNumericAddress)?.lastContactMillis)

            fixture.storeFailing = false
            tick()
            assertEquals(41_000L, fixture.store.loadPairing(fixture.agent::isNumericAddress)?.lastContactMillis)
        }

    @Test
    fun aRevocationThatCannotBeClearedIsShownAndClearedWhenTheStoreWorks() =
        runTest {
            val fixture = Fixture(this)
            assertEquals(PairResult.Paired, fixture.connection().pairByCode(ADDRESS, CODE, "Phone"))
            val observed = fixture.watch()

            fixture.storeFailing = true
            fixture.agent.revoked = true
            tick()
            assertEquals(ComputerLink.StorageError, observed.last().link)
            assertNotNull(fixture.store.read())

            fixture.storeFailing = false
            tick()
            assertEquals(ComputerLink.NotPaired, observed.last().link)
            assertTrue(fixture.store.read() == null)
        }

    @Test
    fun withoutLocalNetworkAccessNothingIsSentAndThePairingIsKept() =
        runTest {
            val fixture = Fixture(this)
            assertEquals(PairResult.Paired, fixture.connection().pairByCode(ADDRESS, CODE, "Phone"))
            val exchanges = fixture.agent.codeExchanges

            fixture.localAccess = false
            val observed = fixture.watch()
            assertEquals(ComputerLink.NetworkPermission, observed.last().link)
            assertEquals(PairResult.NetworkPermission, fixture.connection().pairByCode(ADDRESS, CODE, "Phone"))
            assertEquals(exchanges, fixture.agent.codeExchanges)
            assertTrue(fixture.agent.calls.isEmpty())
            assertNotNull(fixture.store.read())
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
    fun aCancelWhileTheLocalNetworkCheckBlocksPublishesNothingMore() =
        runTest {
            val fixture = Fixture(this)
            assertEquals(PairResult.Paired, fixture.connection().pairByCode(ADDRESS, CODE, "Phone"))
            val observed = fixture.watch()
            assertEquals(ComputerLink.Connected, observed.last().link)

            fixture.agent.action = PhoneReplyKind.Unavailable
            fixture.duringLocalNetworkCheck = { fixture.watching?.cancel() }
            tick()
            assertEquals(ComputerLink.Connected, observed.last().link)
        }

    private fun TestScope.tick() {
        advanceTimeBy(1.seconds)
        runCurrent()
    }

    private class Fixture(
        private val scope: TestScope,
    ) {
        var storeFailing = false
        val store = fakePairingSecret { storeFailing }
        val agent = FakeAgent()
        var network = true
        var localAccess = true
        var localNetworkDenied = false
        var duringLocalNetworkCheck: () -> Unit = {}
        var watching: Job? = null
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
                { localAccess },
                { address ->
                    assertEquals("192.0.2.10", address.host)
                    assertEquals(47821, address.port)
                    duringLocalNetworkCheck()
                    localNetworkDenied
                },
            )

        fun watch(version: Int = PHONE_PROTOCOL): List<LinkSnapshot> {
            val observed = mutableListOf<LinkSnapshot>()
            watching = scope.backgroundScope.launch { connection(version).monitor { observed.add(it) } }
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
        var agentUpdate = false
        var authReply: String? = null
        var codeRequest: Triple<String, Int, String>? = null
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
                        authReply != null && line.contains("\"token\"") -> PhoneReply(PhoneReplyKind.Line, authReply)
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
                    codeRequest = Triple(code, version, name)
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
                if (agentUpdate) {
                    """{"v":1,"error":"agent update needed","agent_proof":"$agentProof"}"""
                } else if (update) {
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
