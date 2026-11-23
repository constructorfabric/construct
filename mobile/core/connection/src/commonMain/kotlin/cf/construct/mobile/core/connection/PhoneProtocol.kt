package cf.construct.mobile.core.connection

import cf.construct.mobile.core.chat.PhoneChannel
import cf.construct.mobile.core.chat.PhoneIdentityMismatch
import cf.construct.mobile.core.chat.PhoneProtocolMismatch
import cf.construct.mobile.core.chat.PhoneReplyKind
import cf.construct.mobile.core.chat.PhoneTransport
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.put

const val PHONE_PROTOCOL = 1

enum class PairResult(
    val message: String?,
) {
    Paired(null),
    InvalidAddress("Enter the address shown on your computer."),
    InvalidCode("Enter the eight-digit code shown on your computer."),
    InvalidQr("Scan a pairing code shown by Construct on your computer."),
    PhoneUpdate("Update this app to connect to the computer."),
    ComputerUpdate("Update Construct on the computer to connect."),
    NoNetwork("This phone has no network connection."),
    NetworkPermission("Allow local network access to connect to your computer."),
    Unreachable("The computer is not reachable. Try again."),
    FreshCode("Request a fresh pairing code on your computer and try again."),
    Refused("The pairing code could not be used. Open a new pairing on your computer."),
    WrongComputer("The computer’s identity could not be verified."),
    StorageError("The pairing could not be saved on this phone."),
}

internal enum class Probe { Connected, Revoked, Unreachable, PhoneUpdate, ComputerUpdate }

internal class PairAttempt(
    val result: PairResult,
    val pairing: StoredPairing? = null,
)

internal class PhoneProtocol(
    private val transport: PhoneTransport,
    private val version: Int,
) {
    fun pair(
        offer: PairingOffer,
        name: String,
    ): PairAttempt =
        try {
            transport.open(offer.address.host, offer.address.port, offer.pin)?.use { channel ->
                val fingerprint = channel.fingerprint
                val phoneName = name.take(MAX_NAME_LENGTH)
                when {
                    !fingerprint.isFingerprint() || (offer.pin != null && offer.pin != fingerprint) -> {
                        PairAttempt(PairResult.WrongComputer)
                    }

                    offer.pin == null -> {
                        confirmCode(offer, channel, phoneName)
                    }

                    else -> {
                        confirmQr(offer, channel, phoneName)
                    }
                }
            } ?: PairAttempt(PairResult.Unreachable)
        } catch (_: PhoneIdentityMismatch) {
            PairAttempt(PairResult.WrongComputer)
        } catch (_: PhoneProtocolMismatch) {
            PairAttempt(PairResult.ComputerUpdate)
        }

    fun open(pairing: StoredPairing): PhoneChannel? =
        try {
            transport.open(pairing.address.host, pairing.address.port, pairing.pin)
        } catch (_: PhoneIdentityMismatch) {
            null
        }

    fun authenticate(
        channel: PhoneChannel,
        pairing: StoredPairing,
    ): Probe =
        probe(
            channel,
            buildJsonObject {
                put("v", version)
                put("token", pairing.token)
                put("watch_revocation", true)
            },
            checkVersion = true,
        )

    fun ping(channel: PhoneChannel): Probe = probe(channel, PING, authenticated = true)

    private fun confirmCode(
        offer: PairingOffer,
        channel: PhoneChannel,
        name: String,
    ): PairAttempt {
        val result = channel.pairCode(offer.secret, version, name)
        val reply = result.line.objectOrNull()
        return when (result.kind) {
            PhoneReplyKind.Refused -> {
                PairAttempt(PairResult.FreshCode)
            }

            PhoneReplyKind.Unavailable, PhoneReplyKind.Closed -> {
                PairAttempt(PairResult.Unreachable)
            }

            PhoneReplyKind.Line -> {
                reply
                    ?.let { evaluatePairing(offer, channel.fingerprint, it) }
                    ?.takeUnless { it.result == PairResult.Refused } ?: PairAttempt(PairResult.Unreachable)
            }
        }
    }

    private fun evaluatePairing(
        offer: PairingOffer,
        fingerprint: String,
        reply: JsonObject,
    ): PairAttempt {
        val update = versionResult(reply)
        val token = reply.string("token")?.takeIf(String::isFingerprint)
        val granted = reply["paired"] == JsonPrimitive(true) && acceptsVersion(reply)
        return when {
            update != null -> PairAttempt(update)
            granted && token != null -> PairAttempt(PairResult.Paired, StoredPairing(offer.address, fingerprint, token))
            else -> PairAttempt(PairResult.Refused)
        }
    }

    private fun confirmQr(
        offer: PairingOffer,
        channel: PhoneChannel,
        name: String,
    ): PairAttempt {
        val fingerprint = channel.fingerprint
        val proof = transport.proof(offer.secret, fingerprint, agent = false) ?: return PairAttempt(PairResult.Refused)
        val hello =
            buildJsonObject {
                put("v", version)
                put(
                    "pair",
                    buildJsonObject {
                        put("proof", proof)
                        put("name", name)
                    },
                )
            }
        val reply = channel.exchange(hello.toString()).line.objectOrNull()
        val agent = transport.proof(offer.secret, fingerprint, agent = true)
        return when {
            reply == null -> PairAttempt(PairResult.Refused)
            !proofsMatch(agent, reply.string("agent_proof")) -> PairAttempt(PairResult.WrongComputer)
            else -> evaluatePairing(offer, fingerprint, reply)
        }
    }

    private fun probe(
        channel: PhoneChannel,
        message: JsonObject,
        checkVersion: Boolean = false,
        authenticated: Boolean = false,
    ): Probe {
        val result = channel.exchange(message.toString())
        val reply = result.line.objectOrNull()
        val update = reply?.takeIf { checkVersion }?.let(::versionResult)
        return when {
            reply == null -> Probe.Unreachable
            authenticated && reply["revoked"] == JsonPrimitive(true) -> Probe.Revoked
            update == PairResult.PhoneUpdate -> Probe.PhoneUpdate
            update == PairResult.ComputerUpdate -> Probe.ComputerUpdate
            checkVersion && !acceptsVersion(reply) -> Probe.Unreachable
            reply["ok"] == JsonPrimitive(true) -> Probe.Connected
            else -> Probe.Unreachable
        }
    }

    private fun acceptsVersion(reply: JsonObject): Boolean = reply.version?.let { it >= version && it > 0 } == true

    private fun versionResult(reply: JsonObject): PairResult? {
        val remote = reply.version ?: return null
        val minimum = (reply["min_version"] as? JsonPrimitive)?.intOrNull
        return when {
            reply.string("error") == "update needed" && minimum != null && version < minimum && minimum <= remote -> {
                PairResult.PhoneUpdate
            }

            reply.string("error") == "agent update needed" && remote < version -> {
                PairResult.ComputerUpdate
            }

            else -> {
                null
            }
        }
    }

    private companion object {
        const val MAX_NAME_LENGTH = 100
        val PING = buildJsonObject { put("cmd", "ping") }
    }
}

private val JsonObject.version: Int?
    get() = (get("v") as? JsonPrimitive)?.takeUnless { it.isString }?.intOrNull

private fun JsonObject.string(name: String): String? = (get(name) as? JsonPrimitive)?.takeIf { it.isString }?.content

private fun String?.objectOrNull(): JsonObject? =
    try {
        this?.let { Json.parseToJsonElement(it) as? JsonObject }
    } catch (_: SerializationException) {
        null
    }

private fun proofsMatch(
    expected: String?,
    actual: String?,
): Boolean {
    if (expected == null || actual == null || expected.length != actual.length) return false
    var difference = 0
    for (index in expected.indices) {
        difference = difference or (expected[index].code xor actual[index].code)
    }
    return difference == 0
}
