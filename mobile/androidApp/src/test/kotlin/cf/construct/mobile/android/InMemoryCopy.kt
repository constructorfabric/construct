package cf.construct.mobile.android

import app.cash.sqldelight.driver.jdbc.sqlite.JdbcSqliteDriver
import cf.construct.mobile.core.database.sql.ConstructDatabase

/** An empty copy in memory, with the schema in place. */
internal fun newInMemoryDriver() =
    JdbcSqliteDriver(JdbcSqliteDriver.IN_MEMORY).also {
        ConstructDatabase.Schema.create(it)
    }
