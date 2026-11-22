package cf.construct.mobile.core.chat

/**
 * The chat core, which is Rust, as the shared Kotlin code of both apps sees it. It is an interface so
 * that the screens' state holders, which call it for the chat, can be tested with a fake.
 */
interface ChatCore {
    /**
     * The request body of the fixed test conversation over [toolsFile], the text of a tool file. The
     * bytes are what the provider's prompt cache sees.
     *
     * @throws ChatCoreException with a plain reason when the tool file is not valid.
     */
    fun fixedRequestBody(toolsFile: String): String

    /**
     * Tests a model setting. The key is an argument: it is in no log and in no returned text.
     * [ModelTestResult.Failed] carries the desktop's words for the reason.
     *
     * Not yet shown on Android for an `https` address: the certificate check may need an initialisation
     * that is not made, and the answer would then be "unreachable". Only `http` on localhost is tested.
     */
    fun testModel(
        baseUrl: String,
        model: String,
        apiKey: String,
    ): ModelTestResult
}

/** A request the core refused, with a fixed plain reason that never holds a key. */
class ChatCoreException(
    message: String,
) : Exception(message)

/** What the Test of a model setting came to. */
sealed interface ModelTestResult {
    /** Every check works; [answer] is what the model said. */
    data class Works(
        val answer: String,
    ) : ModelTestResult

    /** A check failed or the request could not be made; [reason] is the desktop's text. */
    data class Failed(
        val reason: String,
    ) : ModelTestResult
}

/** The chat core of this platform: Rust, loaded as a shared library on Android and linked in on the iPhone. */
fun nativeChatCore(): ChatCore = RustChatCore

private object RustChatCore : ChatCore {
    override fun fixedRequestBody(toolsFile: String): String = nativeFixedRequestBody(toolsFile)

    override fun testModel(
        baseUrl: String,
        model: String,
        apiKey: String,
    ): ModelTestResult = interpretModelTest(nativeTestModel(baseUrl, model, apiKey))
}

/** The Rust side of [ChatCore.fixedRequestBody]. */
internal expect fun nativeFixedRequestBody(toolsFile: String): String

/** The Rust side of [ChatCore.testModel]: the one JSON line the desktop's Settings page gets. */
internal expect fun nativeTestModel(
    baseUrl: String,
    model: String,
    apiKey: String,
): String
