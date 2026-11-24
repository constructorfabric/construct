package cf.construct.mobile.core.keys

fun iosPairingSecret(service: String = "construct.computer"): PairingSecret {
    val value = KeychainSecret(service, "pairing")
    return PairingSecret(value, value::delete)
}
