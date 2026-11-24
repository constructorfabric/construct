package cf.construct.mobile.core.connection

import cf.construct.mobile.core.keys.ModelSettingsException
import cf.construct.mobile.core.keys.PairingSecret

private const val RECORD_FIELDS = 4

/**
 * Address and identity are separate, and committed together with the credential. The time of the last
 * contact is in the same record on purpose: one atomic secure write cannot leave the address, the pin, the
 * token and the time disagreeing, at the price of rewriting the record about every 30 seconds while connected.
 *
 * The record is four lines with no version tag. A record the app cannot read, from a later app or a damaged
 * file, is a storage error and is left as it is until the person pairs again and replaces it.
 */
internal data class StoredPairing(
    val address: ComputerAddress,
    val pin: String,
    val token: String,
    val lastContactMillis: Long = 0,
) {
    override fun toString() = "Saved computer pairing"
}

internal fun PairingSecret.loadPairing(isNumericAddress: (String) -> Boolean): StoredPairing? {
    val record = read()?.split('\n') ?: return null
    if (record.size != RECORD_FIELDS) badRecord()
    val (endpoint, fingerprint, credential) = record
    val address = ComputerAddress.parse(endpoint, isNumericAddress) ?: badRecord()
    val pin = fingerprint.takeIf(String::isFingerprint) ?: badRecord()
    val token = credential.takeIf(String::isFingerprint) ?: badRecord()
    val time = record.last().toLongOrNull()?.takeIf { it >= 0 } ?: badRecord()
    return StoredPairing(address, pin, token, time)
}

internal fun PairingSecret.savePairing(pairing: StoredPairing) {
    save("${pairing.address.endpoint}\n${pairing.pin}\n${pairing.token}\n${pairing.lastContactMillis}")
}

private fun badRecord(): Nothing = throw ModelSettingsException("The saved computer pairing cannot be opened.")
