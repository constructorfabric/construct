package cf.construct.mobile.core.database

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.construct.FakeConstruct
import cf.construct.mobile.core.model.DocumentState
import cf.construct.mobile.core.model.MemoryFacts
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.test.TestScope

internal fun seededStore(): CopyStore {
    val store = CopyStore(newInMemoryDriver())
    FakeConstruct.seededConversations.forEach { (summary, messages) -> store.saveConversation(summary, messages) }
    store.saveFacts(MemoryFacts(FakeConstruct.seededFacts, FakeConstruct.FACTS_VERSION))
    store.saveDocument(DocumentState(FakeConstruct.seededDocument, null))
    store.saveNumbers(contractInput, contractNumbers)
    return store
}

class DatabaseConstructContractTest : ConstructContract() {
    override suspend fun TestScope.construct(): Construct = DatabaseConstruct(seededStore(), Dispatchers.Unconfined)
}
