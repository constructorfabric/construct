package cf.construct.mobile.core.database

import app.cash.sqldelight.db.SqlDriver
import app.cash.sqldelight.driver.native.NativeSqliteDriver
import cf.construct.mobile.core.database.sql.ConstructDatabase

fun nativeCopyDriver(name: String = COPY_FILE): SqlDriver = NativeSqliteDriver(ConstructDatabase.Schema, name)
