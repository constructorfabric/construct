package cf.construct.mobile.core.chat

import app.cash.sqldelight.db.SqlDriver
import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.core.database.CopyStore
import cf.construct.mobile.core.database.PhoneConstruct
import cf.construct.mobile.core.keys.FakeModelSettingsStore
import cf.construct.mobile.core.model.ChatEvent
import cf.construct.mobile.core.model.ChatInput
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.core.model.Outcome
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue

/** The phone's Construct, SQLite and the native core against a local fake provider. */
abstract class PhoneConstructTestCases {
    abstract val fakes: Fakes

    abstract fun openCopy(): SqlDriver

    open fun closeCopy(driver: SqlDriver) = driver.close()

    @Test
    fun streamsChangesToFactsAndKeepsThePartialAnswer() =
        withCopy { chat, sql, settings ->
            sql("INSERT INTO fact VALUES ('f1', 'IDENTITY', 'name', '\"Ada\"', '2026-01-01T00:00:00Z', NULL)")
            val partial = Reply.Stream(delta("partial"), holdOpen = true)
            fakes.start(listOf(complete, complete, complete, partial)).use { provider ->
                settings.saveProvider(provider.baseUrl, "local-model", FAKE_API_KEY)
                chat.events { events ->
                    val id = answer(chat, events, "", "one")
                    answer(chat, events, id, "two")
                    sql("UPDATE fact SET value_json = '\"Lin\"' WHERE fact_id = 'f1'")
                    answer(chat, events, id, "three")
                    assertEquals(Outcome.Ok(Unit), chat.sendMessage(ChatInput(id, "four")))
                    assertEquals(ChatEvent.Start(id), events.receive())
                    assertEquals(ChatEvent.Delta("partial"), events.receive())
                    chat.stopAnswer()
                    assertEquals(ChatEvent.Done(stopped = true), events.receive())

                    val messages = assertIs<Outcome.Ok<List<ChatMessage>>>(chat.getConversation(id)).value
                    assertEquals(
                        listOf(
                            "one",
                            "first second",
                            "two",
                            "first second",
                            "three",
                            "first second",
                            "four",
                            "partial",
                        ),
                        messages.map { it.text },
                    )
                    assertEquals(ChatRole.ASSISTANT, messages.last().role)
                    val bodies = provider.requests.map { it.body }
                    assertEquals(List(4) { "Bearer fake-key" }, provider.requests.map { it.authorization })
                    assertEquals(listOf(1, 1, 2, 2), bodies.map(::recallCount))
                    assertGolden(bodies.first())
                    assertTrue(bodies[0].contains("identity.name: Ada"))
                    assertTrue(bodies[2].contains("identity.name: Lin"))
                    assertEquals(Outcome.Ok(Unit), chat.deleteConversation(id))
                    assertIs<Outcome.Failed>(chat.getConversation(id))
                }
            }
        }

    @Test
    fun aStalledCollectorDoesNotKeepTheCompletedAnswerBusy() =
        withCopy { chat, _, settings ->
            fakes.start(listOf(complete)).use { provider ->
                settings.saveProvider(provider.baseUrl, "local-model", FAKE_API_KEY)
                coroutineScope {
                    val started = CompletableDeferred<String>()
                    val collector =
                        launch(start = CoroutineStart.UNDISPATCHED) {
                            chat.chatEvents.collect { event ->
                                if (event is ChatEvent.Start) {
                                    started.complete(event.conversation)
                                    CompletableDeferred<Unit>().await()
                                }
                            }
                        }
                    try {
                        assertEquals(Outcome.Ok(Unit), chat.sendMessage(ChatInput("", "one")))
                        val id = started.await()
                        var deletion = chat.deleteConversation(id)
                        while (deletion == Outcome.Failed("busy")) {
                            delay(10)
                            deletion = chat.deleteConversation(id)
                        }
                        assertEquals(Outcome.Ok(Unit), deletion)
                        assertIs<Outcome.Failed>(chat.getConversation(id))
                    } finally {
                        collector.cancel()
                    }
                }
            }
        }

    @Test
    fun aFullEventQueueDeliversOneOverflowErrorAndAcceptsAnotherSend() =
        withCopy { chat, _, settings ->
            val overflowing = Reply.Stream(delta("piece").repeat(128), holdOpen = true)
            fakes.start(listOf(overflowing, complete)).use { provider ->
                settings.saveProvider(provider.baseUrl, "local-model", FAKE_API_KEY)
                val started = CompletableDeferred<String>()
                val resume = CompletableDeferred<Unit>()
                chat.events(beforeForward = {
                    if (it is ChatEvent.Start && started.complete(it.conversation)) resume.await()
                }) { events ->
                    assertEquals(Outcome.Ok(Unit), chat.sendMessage(ChatInput("", "one")))
                    val id = started.await()
                    // The native core saves the partial answer when the full queue stops the stream.
                    while (assertIs<Outcome.Ok<List<ChatMessage>>>(chat.getConversation(id)).value.last().role !=
                        ChatRole.ASSISTANT
                    ) {
                        delay(10)
                    }
                    resume.complete(Unit)
                    assertEquals(ChatEvent.Start(id), events.receive())
                    var delivered = 0
                    var next = events.receive()
                    while (next == ChatEvent.Delta("piece")) {
                        delivered++
                        next = events.receive()
                    }
                    // The actor can take the start and one delta off the queue before it blocks.
                    assertTrue(delivered in 62..64, "delivered $delivered deltas")
                    assertEquals(ChatEvent.Error("the chat event stream fell behind"), next)

                    // The next event must be the next turn's start, with no duplicate error or done.
                    assertEquals(id, answer(chat, events, id, "two"))
                    assertEquals(2, provider.requests.size)
                }
            }
        }

    @Test
    fun computerConversationsStayReadOnlyAndSurviveDeleteAll() =
        withCopy { chat, sql, settings ->
            sql("INSERT INTO conversation(id, title, time) VALUES ('computer', 'Desktop', '2026-01-01T00:00:00Z')")
            sql("INSERT INTO message VALUES ('computer', 0, 'USER', 'Kept')")
            val readOnly = Outcome.Failed("this conversation was started on the computer and is read-only")
            assertEquals(readOnly, chat.sendMessage(ChatInput("computer", "Hi")))
            assertEquals(readOnly, chat.deleteConversation("computer"))
            fakes.start(listOf(complete, complete)).use { provider ->
                settings.saveProvider(provider.baseUrl, "local-model", FAKE_API_KEY)
                chat.events { events ->
                    val first = answer(chat, events, "", "one")
                    val second = answer(chat, events, "", "two")
                    assertEquals(Outcome.Ok(Unit), chat.deleteConversation("all"))
                    assertIs<Outcome.Failed>(chat.getConversation(first))
                    assertIs<Outcome.Failed>(chat.getConversation(second))
                }
            }
            assertEquals(Outcome.Ok(listOf(ChatMessage(ChatRole.USER, "Kept"))), chat.getConversation("computer"))
        }

    @Test
    fun givesPlainProviderFailuresThroughTheEventFlow() =
        withCopy { chat, _, settings ->
            chat.events { events ->
                assertEquals(Outcome.Failed("no model is set"), chat.sendMessage(ChatInput("", "Hello")))
                assertEquals(ChatEvent.Error("no model is set"), events.receive())

                settings.saveProvider(fakes.closedAddress(), "local-model", FAKE_API_KEY)
                assertEquals(Outcome.Ok(Unit), chat.sendMessage(ChatInput("", "Hello")))
                assertIs<ChatEvent.Start>(events.receive())
                assertEquals(ChatEvent.Error("unreachable"), events.receive())

                val tooLong = Reply.Status(400, """{"error":{"code":"context_length_exceeded"}}""")
                fakes.start(listOf(Reply.Status(401, "{}"), Reply.Status(404, "{}"), tooLong)).use { provider ->
                    settings.saveProvider(provider.baseUrl, "local-model", FAKE_API_KEY)
                    for (reason in listOf("key refused", "model not found", "too long")) {
                        assertEquals(Outcome.Ok(Unit), chat.sendMessage(ChatInput("", "Hello")))
                        assertIs<ChatEvent.Start>(events.receive())
                        assertEquals(ChatEvent.Error(reason), events.receive())
                    }
                }
            }
        }

    @Test
    fun givesTheFirstResponseDeadlineAsATimeout() =
        withCopy(timeoutMillis = 75_000) { chat, _, settings ->
            fakes.start(listOf(Reply.Hang)).use { provider ->
                settings.saveProvider(provider.baseUrl, "local-model", FAKE_API_KEY)
                chat.events { events ->
                    assertEquals(Outcome.Ok(Unit), chat.sendMessage(ChatInput("", "Hello")))
                    assertIs<ChatEvent.Start>(events.receive())
                    assertEquals(ChatEvent.Error("timeout"), events.receive())
                }
            }
        }

    @Test
    fun refusesAnOversizedTurnBeforeWritingItOrCallingTheProvider() =
        withCopy { chat, _, settings ->
            fakes.start(listOf(complete)).use { provider ->
                settings.saveProvider(provider.baseUrl, "local-model", FAKE_API_KEY)
                chat.events { events ->
                    val huge = "é".repeat(524_288)
                    assertEquals(Outcome.Ok(Unit), chat.sendMessage(ChatInput("", huge)))
                    assertEquals(ChatEvent.Error("request too large"), events.receive())
                    assertEquals(Outcome.Ok(emptyList()), chat.listConversations())
                    val id = answer(chat, events, "", "one")
                    assertEquals(1, provider.requests.size)
                    assertEquals(Outcome.Ok(Unit), chat.deleteConversation(id))
                }
            }
        }

    private fun withCopy(
        timeoutMillis: Long = 30_000,
        block: suspend (Construct, (String) -> Unit, FakeModelSettingsStore) -> Unit,
    ) = runBlocking {
        val driver = openCopy()
        val settings = FakeModelSettingsStore()
        val chat = PhoneConstruct(CopyStore(driver), settings, Dispatchers.Default)
        try {
            withTimeout(timeoutMillis) {
                block(chat, { driver.execute(null, it, 0) }, settings)
            }
        } finally {
            chat.eraseEverything()
            closeCopy(driver)
        }
    }

    private suspend fun Construct.events(
        beforeForward: suspend (ChatEvent) -> Unit = {},
        block: suspend (Channel<ChatEvent>) -> Unit,
    ) = coroutineScope {
        val channel = Channel<ChatEvent>(Channel.UNLIMITED)
        val collector =
            launch(start = CoroutineStart.UNDISPATCHED) {
                chatEvents.collect {
                    beforeForward(it)
                    channel.send(it)
                }
            }
        try {
            block(channel)
        } finally {
            collector.cancel()
            channel.close()
        }
    }

    private suspend fun answer(
        chat: Construct,
        events: Channel<ChatEvent>,
        id: String,
        text: String,
    ): String {
        assertEquals(Outcome.Ok(Unit), chat.sendMessage(ChatInput(id, text)))
        val start = assertIs<ChatEvent.Start>(events.receive())
        assertEquals(ChatEvent.Delta("first"), events.receive())
        assertEquals(ChatEvent.Delta(" second"), events.receive())
        assertEquals(ChatEvent.Done(stopped = false), events.receive())
        return start.conversation
    }

    private fun recallCount(body: String): Int =
        Json.parseToJsonElement(body).jsonObject.getValue("messages").jsonArray.sumOf { message ->
            message.jsonObject["tool_calls"]?.jsonArray.orEmpty().count {
                val function = it.jsonObject.getValue("function").jsonObject
                function.getValue("name").jsonPrimitive.content == "recall_facts"
            }
        }

    private fun assertGolden(body: String) {
        val messages =
            Json
                .parseToJsonElement(body)
                .jsonObject
                .getValue("messages")
                .jsonArray
        val recall =
            messages
                .last()
                .jsonObject
                .getValue("tool_call_id")
                .jsonPrimitive.content
        assertTrue(Regex("call_[0-9a-f]{32}").matches(recall))
        val fixture = fixtureText("chat-core/tests/golden/phone-live.json").trimEnd('\n')
        assertEquals(fixture.replace("call_golden_recall", recall), body)
    }
}

private const val FAKE_API_KEY = "fake-key"

private val complete =
    Reply.Stream(
        delta("first") + delta(" second") +
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    )

private fun delta(text: String): String = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"$text\"}}]}\n\n"

private inline fun FakeProvider.use(block: (FakeProvider) -> Unit) {
    try {
        block(this)
    } finally {
        close()
    }
}
