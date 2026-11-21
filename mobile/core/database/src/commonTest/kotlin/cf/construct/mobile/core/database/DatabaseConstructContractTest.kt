package cf.construct.mobile.core.database

import cf.construct.mobile.core.construct.Construct
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.test.TestScope

internal fun seededStore(): CopyStore {
    val store = CopyStore(newInMemoryDriver())
    store.seedWithSampleData()
    store.saveNumbers(contractInput, contractNumbers)
    return store
}

class DatabaseConstructContractTest : ConstructContract() {
    override suspend fun TestScope.construct(): Construct = DatabaseConstruct(seededStore(), Dispatchers.Unconfined)
}
