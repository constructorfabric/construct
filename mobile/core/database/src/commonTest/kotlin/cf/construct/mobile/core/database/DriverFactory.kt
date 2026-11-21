package cf.construct.mobile.core.database

import app.cash.sqldelight.db.SqlDriver

expect fun newInMemoryDriver(): SqlDriver
