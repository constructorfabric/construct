package cf.construct.mobile.shared

import cf.construct.mobile.core.connection.PhoneConnection
import cf.construct.mobile.core.database.CopyStore

/** Keep database and Keychain failures inside Kotlin until the Swift error bridge. */
@Throws(Exception::class)
suspend fun unpairComputer(
    connection: PhoneConnection,
    copy: CopyStore,
) = connection.unpair(copy::removeComputerCopy)
