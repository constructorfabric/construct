package cf.construct.mobile.core.database

import android.content.Context
import app.cash.sqldelight.db.SqlDriver
import app.cash.sqldelight.driver.android.AndroidSqliteDriver
import cf.construct.mobile.core.database.sql.ConstructDatabase

fun androidCopyDriver(
    context: Context,
    name: String = COPY_FILE,
): SqlDriver = AndroidSqliteDriver(ConstructDatabase.Schema, context, name)
