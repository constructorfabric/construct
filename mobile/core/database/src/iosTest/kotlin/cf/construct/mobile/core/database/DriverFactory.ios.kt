package cf.construct.mobile.core.database

import app.cash.sqldelight.db.SqlDriver
import app.cash.sqldelight.driver.native.inMemoryDriver
import cf.construct.mobile.core.database.sql.ConstructDatabase

actual fun newInMemoryDriver(): SqlDriver = inMemoryDriver(ConstructDatabase.Schema)
