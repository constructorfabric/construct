package cf.construct.mobile.core.chat

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

private const val FAKE_KEY = "sk-fake-key-4f9a2c71"
private const val MODEL = "fake-model"

private const val COMPLETION = """{"choices":[{"message":{"content":"pong"}}]}"""
private const val TOOL_CALL =
    """{"choices":[{"message":{"content":null,"tool_calls":[{"id":"call_1","type":"function",""" +
        """"function":{"name":"add","arguments":"{\"a\":2,\"b\":3}"}}]}}]}"""
private const val OTHER_TOOL_CALL =
    """{"choices":[{"message":{"content":null,"tool_calls":[{"id":"call_1","type":"function",""" +
        """"function":{"name":"remove","arguments":"{}"}}]}}]}"""
private const val SUM = """{"choices":[{"message":{"content":"The sum is 5."}}]}"""
private const val WRONG_SUM = """{"choices":[{"message":{"content":"The sum is 7."}}]}"""

/** The provider's error text echoes the key, as a careless one does: it must never come back. */
private val echoesKey = """{"error":"bad key $FAKE_KEY"}"""

/**
 * The tests of the chat core, run through the real Rust library on each platform: the emulator and
 * the iPhone Simulator each have a class that gives their [Fakes].
 */
abstract class ChatCoreTestCases {
    protected abstract val fakes: Fakes

    private val core = nativeChatCore()

    /** Runs the Test against a fake provider that gives [replies], and checks the key stays out. */
    private fun testAgainst(vararg replies: Reply): ModelTestResult {
        val provider = fakes.start(replies.toList())
        try {
            val result = core.testModel(provider.baseUrl, MODEL, FAKE_KEY)
            assertFalse(result.toString().contains(FAKE_KEY), "the key is in the result: $result")
            return result
        } finally {
            provider.close()
        }
    }

    private fun failed(result: ModelTestResult): String {
        assertTrue(result is ModelTestResult.Failed, "expected a failure, got $result")
        return result.reason
    }

    @Test
    fun theFixedConversationGivesTheStoredPhoneBody() {
        val stored = fixtureText("chat-core/tests/golden/phone.json").removeSuffix("\n")
        assertEquals(stored, core.fixedRequestBody(fixtureText("tools/phone.json")))
    }

    @Test
    fun theFixedConversationGivesTheStoredDesktopBody() {
        val stored = fixtureText("chat-core/tests/golden/desktop.json").removeSuffix("\n")
        assertEquals(stored, core.fixedRequestBody(fixtureText("tools/desktop.json")))
    }

    @Test
    fun aBadToolFileGivesAPlainReason() {
        val error = assertFailsWith<ChatCoreException> { core.fixedRequestBody("not json") }
        assertTrue(error.message.orEmpty().startsWith("bad tool file"), error.message)
    }

    @Test
    fun aProviderThatAnswersWorks() {
        val provider =
            fakes.start(
                listOf(Reply.Status(200, COMPLETION), Reply.Status(200, TOOL_CALL), Reply.Status(200, SUM)),
            )
        try {
            assertEquals(ModelTestResult.Works("pong"), core.testModel(provider.baseUrl, MODEL, FAKE_KEY))
            assertEquals(3, provider.bodies.size)
            assertTrue(provider.bodies.all { it.contains("\"$MODEL\"") })
        } finally {
            provider.close()
        }
    }

    @Test
    fun aKeyTheProviderRefusesIsKeyRefused() {
        assertEquals(
            "key refused: the provider did not accept the key.",
            failed(testAgainst(Reply.Status(401, echoesKey))),
        )
    }

    @Test
    fun aModelTheProviderDoesNotKnowIsModelNotFound() {
        assertEquals(
            "model not found: the provider has no model with this name.",
            failed(testAgainst(Reply.Status(404, echoesKey))),
        )
    }

    @Test
    fun aProviderThatCannotBeReachedIsUnreachable() {
        val result = core.testModel(fakes.closedAddress(), MODEL, FAKE_KEY)
        assertEquals("unreachable: nothing answered at this address.", failed(result))
        assertFalse(result.toString().contains(FAKE_KEY))
    }

    @Test
    fun aProviderThatDoesNotAnswerInTimeIsATimeout() {
        assertEquals(
            "timeout: the provider did not answer in time.",
            failed(testAgainst(Reply.Hang)),
        )
    }

    @Test
    fun anOtherStatusIsNamed() {
        assertEquals(
            "error: the provider answered with status 503.",
            failed(testAgainst(Reply.Status(503, echoesKey))),
        )
    }

    @Test
    fun aProviderThatRefusesToolsIsToolsRefused() {
        assertEquals(
            "tools refused: the provider did not accept a request that offers tools.",
            failed(testAgainst(Reply.Status(200, COMPLETION), Reply.Status(400, echoesKey))),
        )
    }

    @Test
    fun aModelThatAnswersInTextWhereItMustCallTheToolIsNoToolCall() {
        assertEquals(
            "no tool call: the model answered in text where it had to call the tool.",
            failed(testAgainst(Reply.Status(200, COMPLETION), Reply.Status(200, COMPLETION))),
        )
    }

    @Test
    fun aModelThatCallsAnotherToolIsBadToolCall() {
        assertEquals(
            "bad tool call: the model called a tool in a way that does not fit.",
            failed(testAgainst(Reply.Status(200, COMPLETION), Reply.Status(200, OTHER_TOOL_CALL))),
        )
    }

    @Test
    fun aModelThatDoesNotAnswerFromTheResultIsWrongAnswer() {
        assertEquals(
            "wrong answer: the model did not answer from the result of the tool.",
            failed(
                testAgainst(Reply.Status(200, COMPLETION), Reply.Status(200, TOOL_CALL), Reply.Status(200, WRONG_SUM)),
            ),
        )
    }

    @Test
    fun aPlainHttpAddressThatIsNotLocalIsRefusedAndNothingIsSent() {
        val refused =
            "Not done: base_url must be an https address, or http on localhost or 127.0.0.1, " +
                "without a user name or password"
        assertEquals(refused, failed(core.testModel("http://models.example.com/v1", MODEL, FAKE_KEY)))
        assertEquals(refused, failed(core.testModel("http://192.168.1.20:8080", MODEL, FAKE_KEY)))
    }

    @Test
    fun aKeyThatIsNotAValidHeaderIsRefusedWithoutItsText() {
        val bad = "sk-fake\nkey-4f9a2c71"
        val result = core.testModel("https://models.example.com/v1", MODEL, bad)
        assertFalse(result.toString().contains("4f9a2c71"), result.toString())
        assertTrue(result is ModelTestResult.Failed)
    }
}
