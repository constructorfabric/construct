package cf.construct.mobile.core.database

import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.construct.FakeConstruct
import kotlinx.coroutines.test.TestScope

class FakeConstructContractTest : ConstructContract() {
    override suspend fun TestScope.construct(): Construct = FakeConstruct(this)
}
