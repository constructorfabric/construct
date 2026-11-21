package cf.construct.mobile.core.database

import app.cash.sqldelight.db.SqlDriver
import app.cash.sqldelight.driver.jdbc.sqlite.JdbcSqliteDriver
import cf.construct.mobile.core.database.sql.ConstructDatabase

actual fun newInMemoryDriver(): SqlDriver =
    JdbcSqliteDriver(JdbcSqliteDriver.IN_MEMORY).also { ConstructDatabase.Schema.create(it) }
