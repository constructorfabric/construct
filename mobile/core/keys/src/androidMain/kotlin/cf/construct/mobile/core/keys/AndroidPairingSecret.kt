package cf.construct.mobile.core.keys

import android.content.Context
import java.io.File

fun androidPairingSecret(context: Context): PairingSecret {
    val path = File(context.applicationContext.noBackupFilesDir, "computer_pairing.bin")
    val value = KeystoreFileSecret(path) { keystoreAesKey("construct.computer.pairing") }
    return PairingSecret(value) {
        if (!path.delete() && path.exists()) {
            throw ModelSettingsException("The pairing could not be removed from this phone.")
        }
    }
}
