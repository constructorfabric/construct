package cf.construct.mobile.shared

import cf.construct.mobile.core.connection.PhoneConnection
import cf.construct.mobile.core.database.CopyStore
import cf.construct.mobile.core.sync.SyncEngine
import cf.construct.mobile.core.sync.Syncer

/**
 * The sync of the iPhone app: it copies from the paired computer into [copy]. Swift cannot use the engine's own
 * constructor, whose other arguments have defaults that Swift does not see.
 */
fun iosSyncer(
    connection: PhoneConnection,
    copy: CopyStore,
): Syncer = SyncEngine(connection, copy)
