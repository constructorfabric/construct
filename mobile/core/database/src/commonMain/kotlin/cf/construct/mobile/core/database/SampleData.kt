package cf.construct.mobile.core.database

import cf.construct.mobile.core.construct.FakeConstruct
import cf.construct.mobile.core.model.DocumentState
import cf.construct.mobile.core.model.MemoryFacts

/**
 * Fills an empty copy with the data [FakeConstruct] starts with, so a build that has no sync yet
 * has something to show. A copy that already has a profile is left as it is.
 *
 * @return whether the copy was filled.
 */
fun CopyStore.seedWithSampleData(): Boolean {
    if (memoryVersion() != null) return false
    FakeConstruct.seededConversations.forEach { (summary, messages) -> saveConversation(summary, messages) }
    saveFacts(MemoryFacts(FakeConstruct.seededFacts, FakeConstruct.FACTS_VERSION))
    saveDocument(DocumentState(FakeConstruct.seededDocument, null))
    return true
}
