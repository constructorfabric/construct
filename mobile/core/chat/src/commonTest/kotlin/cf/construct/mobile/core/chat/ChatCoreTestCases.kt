package cf.construct.mobile.core.chat

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
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
            val result = core.testModel(provider.baseUrl, MODEL, FAKE_KEY)
            assertEquals(ModelTestResult.Works("pong"), result)
            assertFalse(result.toString().contains(FAKE_KEY))
            assertEquals(3, provider.requests.size)
            // The key reaches the provider, as the bearer token of every request, and the model name too.
            assertTrue(provider.requests.all { it.authorization == "Bearer $FAKE_KEY" })
            assertTrue(provider.requests.all { it.body.contains("\"$MODEL\"") })
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
    fun aKeyThatIsNotAValidHeaderIsRefusedWithItsFixedTextAndNothingIsSent() {
        val provider = fakes.start(listOf(Reply.Status(200, COMPLETION)))
        try {
            val bad = "sk-fake\nkey-4f9a2c71"
            val result = core.testModel(provider.baseUrl, MODEL, bad)
            assertEquals(ModelTestResult.Failed("Not done: invalid api_key"), result)
            assertTrue(provider.requests.isEmpty())
        } finally {
            provider.close()
        }
    }

    @Test
    fun theReasonTextsAreTheDesktopTextsOfItsSettingsPage() {
        val desktop = Json.parseToJsonElement(fixtureText("app/src/en.json")).jsonObject

        fun text(key: String) = desktop.getValue(key).jsonPrimitive.content
        val keys =
            mapOf(
                "key refused" to "OutcomeMessage.KeyRefused",
                "model not found" to "OutcomeMessage.ModelNotFound",
                "unreachable" to "OutcomeMessage.Unreachable",
                "timeout" to "OutcomeMessage.Timeout",
                "tools refused" to "OutcomeMessage.ToolsRefused",
                "no tool call" to "OutcomeMessage.NoToolCall",
                "bad tool call" to "OutcomeMessage.BadToolCall",
                "wrong answer" to "OutcomeMessage.WrongAnswer",
            )
        assertEquals(keys.keys, desktopReasons.keys)
        keys.forEach { (result, key) -> assertEquals(text(key), desktopReasons[result], result) }
        // The two texts with a value in them, filled in the way `interpretModelTest` fills them.
        assertEquals(
            text("OutcomeMessage.Status").replace("{{status}}", "503"),
            "error: the provider answered with status 503.",
        )
        assertEquals("Not done: {{reason}}", text("OutcomeMessage.Refused"))
    }

    @Test
    fun theGeneratedFixturesAreTheFilesByteForByte() {
        // The length and the byte sum come from the files themselves, not from the escaped literals.
        GeneratedFixtures.measures.forEach { (name, measure) ->
            val bytes = fixtureText(name).encodeToByteArray()
            assertEquals(measure.bytes, bytes.size, name)
            assertEquals(measure.sum, bytes.sumOf { it.toInt() and 0xFF }, name)
        }
        assertEquals(GeneratedFixtures.files.keys, GeneratedFixtures.measures.keys)
    }
}
