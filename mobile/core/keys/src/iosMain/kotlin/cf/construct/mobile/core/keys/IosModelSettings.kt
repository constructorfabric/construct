@file:OptIn(ExperimentalForeignApi::class, BetaInteropApi::class)

package cf.construct.mobile.core.keys

import kotlinx.cinterop.BetaInteropApi
import kotlinx.cinterop.ByteVar
import kotlinx.cinterop.CPointer
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.alloc
import kotlinx.cinterop.convert
import kotlinx.cinterop.memScoped
import kotlinx.cinterop.ptr
import kotlinx.cinterop.reinterpret
import kotlinx.cinterop.value
import platform.CoreFoundation.CFDictionaryAddValue
import platform.CoreFoundation.CFDictionaryCreateMutable
import platform.CoreFoundation.CFDictionaryRef
import platform.CoreFoundation.CFRelease
import platform.CoreFoundation.CFTypeRefVar
import platform.CoreFoundation.kCFBooleanTrue
import platform.CoreFoundation.kCFTypeDictionaryKeyCallBacks
import platform.CoreFoundation.kCFTypeDictionaryValueCallBacks
import platform.Foundation.CFBridgingRelease
import platform.Foundation.CFBridgingRetain
import platform.Foundation.NSData
import platform.Foundation.NSString
import platform.Foundation.NSUTF8StringEncoding
import platform.Foundation.NSUserDefaults
import platform.Foundation.create
import platform.Foundation.dataUsingEncoding
import platform.Security.SecItemAdd
import platform.Security.SecItemCopyMatching
import platform.Security.SecItemDelete
import platform.Security.SecItemUpdate
import platform.Security.errSecItemNotFound
import platform.Security.errSecSuccess
import platform.Security.kSecAttrAccessible
import platform.Security.kSecAttrAccessibleWhenUnlockedThisDeviceOnly
import platform.Security.kSecAttrAccount
import platform.Security.kSecAttrService
import platform.Security.kSecAttrSynchronizable
import platform.Security.kSecClass
import platform.Security.kSecClassGenericPassword
import platform.Security.kSecMatchLimit
import platform.Security.kSecMatchLimitOne
import platform.Security.kSecReturnData
import platform.Security.kSecValueData

private const val SERVICE = "construct.model"
private const val ACCOUNT = "provider-settings"
private const val LEGACY_ACCOUNT = "api-key"

/**
 * The model setting of an iPhone is one Keychain item that stays on this device.
 * Preferences supply the address and model from releases that stored the key separately.
 */
fun iosModelSettingsStore(
    defaults: NSUserDefaults = NSUserDefaults.standardUserDefaults,
    service: String = SERVICE,
): ModelSettingsStore =
    StoredModelSettings(
        TextValues(defaults::stringForKey),
        KeychainSecret(service, ACCOUNT),
    ) { KeychainSecret(service, LEGACY_ACCOUNT).delete() }

/** One generic-password item of the Keychain. Its text is never part of an error. */
class KeychainSecret(
    private val service: String,
    private val account: String,
) : SecretValue {
    override fun read(): String? {
        val (status, data) = fetch()
        return when (status) {
            errSecSuccess -> {
                data?.let { NSString.create(it, NSUTF8StringEncoding)?.toString() }
                    ?: throw ModelSettingsException("The saved key cannot be opened. Enter it again.")
            }

            errSecItemNotFound -> {
                null
            }

            else -> {
                throw ModelSettingsException("The saved key cannot be opened. Enter it again.")
            }
        }
    }

    override fun write(secret: String) {
        val bytes = (NSString.create(string = secret).dataUsingEncoding(NSUTF8StringEncoding))
        val data = CFBridgingRetain(bytes)
        val attributes =
            CFDictionaryCreateMutable(null, 0, kCFTypeDictionaryKeyCallBacks.ptr, kCFTypeDictionaryValueCallBacks.ptr)
        val status =
            try {
                CFDictionaryAddValue(attributes, kSecAttrAccessible, kSecAttrAccessibleWhenUnlockedThisDeviceOnly)
                CFDictionaryAddValue(attributes, kSecValueData, data)
                val updated = withQuery { SecItemUpdate(it, attributes) }
                if (updated == errSecItemNotFound) {
                    withQuery { query ->
                        CFDictionaryAddValue(query, kSecAttrAccessible, kSecAttrAccessibleWhenUnlockedThisDeviceOnly)
                        CFDictionaryAddValue(query, kSecValueData, data)
                        SecItemAdd(query, null)
                    }
                } else {
                    updated
                }
            } finally {
                CFRelease(attributes)
                CFRelease(data)
            }
        if (status != errSecSuccess) throw ModelSettingsException("The key could not be saved on this phone.")
    }

    internal fun delete() {
        val status = withQuery { SecItemDelete(it) }
        if (status != errSecSuccess && status != errSecItemNotFound) {
            throw ModelSettingsException("The old saved key could not be removed. Try again.")
        }
    }

    private fun fetch(): Pair<Int, NSData?> =
        memScoped {
            val found = alloc<CFTypeRefVar>()
            val status =
                withQuery { query ->
                    CFDictionaryAddValue(query, kSecMatchLimit, kSecMatchLimitOne)
                    CFDictionaryAddValue(query, kSecReturnData, kCFBooleanTrue)
                    SecItemCopyMatching(query, found.ptr)
                }
            if (status != errSecSuccess) {
                status to null
            } else {
                status to (CFBridgingRelease(found.value) as NSData?)
            }
        }

    /** Runs [call] with a query that names this item and the one place it is looked for. */
    private fun withQuery(call: (CFDictionaryRef) -> Int): Int {
        val query =
            CFDictionaryCreateMutable(
                null,
                0,
                kCFTypeDictionaryKeyCallBacks.ptr,
                kCFTypeDictionaryValueCallBacks.ptr,
            )
        val serviceRef = CFBridgingRetain(service)
        val accountRef = CFBridgingRetain(account)
        try {
            CFDictionaryAddValue(query, kSecClass, kSecClassGenericPassword)
            CFDictionaryAddValue(query, kSecAttrService, serviceRef)
            CFDictionaryAddValue(query, kSecAttrAccount, accountRef)
            // Only the item on this device, never one that iCloud Keychain brought in.
            CFDictionaryAddValue(query, kSecAttrSynchronizable, platform.CoreFoundation.kCFBooleanFalse)
            return call(query!!)
        } finally {
            CFRelease(serviceRef)
            CFRelease(accountRef)
            CFRelease(query)
        }
    }
}
