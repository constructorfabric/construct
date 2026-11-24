package cf.construct.mobile.android

import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performScrollToKey
import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.ChatRole
import cf.construct.mobile.feature.chat.ChatReply
import cf.construct.mobile.feature.chat.ChatState
import cf.construct.mobile.feature.chat.ChatThread
import cf.construct.mobile.feature.chat.ChatThreadActions
import cf.construct.mobile.feature.chat.ChatThreadScreen
import org.junit.Rule
import org.junit.Test

class ChatThreadScrollTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun newRowsStayVisibleAtTheEndAndLeaveAnOlderMessageInPlace() {
        val messages = List(40) { ChatMessage(ChatRole.USER, "Earlier message $it") }
        val thread = mutableStateOf(ChatThread(id = "phone-chat", messages = messages))
        val answering = mutableStateOf(true)
        val actions = ChatThreadActions({}, {}, {}, {}, {}, {}, {})
        compose.setContent {
            ConstructTheme {
                ChatThreadScreen(ChatState(modelReady = true, answering = answering.value), thread.value, actions)
            }
        }

        compose.onNodeWithText("Earlier message 39").assertIsDisplayed()
        compose.runOnIdle {
            val firstReply = ChatMessage(ChatRole.ASSISTANT, "The first streamed row")
            thread.value = thread.value.copy(messages = messages + firstReply)
        }
        compose.onNodeWithText("The first streamed row").assertIsDisplayed()
        compose.runOnIdle {
            thread.value = thread.value.copy(reply = ChatReply.Stopped)
            answering.value = false
        }
        compose.onNodeWithText("Stopped. The part that was written is kept.").assertIsDisplayed()

        compose.onNode(hasScrollAction()).performScrollToKey(20)
        compose.onNodeWithText("Earlier message 20").assertIsDisplayed()
        compose.runOnIdle {
            thread.value =
                thread.value.copy(
                    messages = thread.value.messages + ChatMessage(ChatRole.TOOL, "New tool row"),
                    reply = null,
                )
            answering.value = true
        }
        compose.onNodeWithText("Earlier message 20").assertIsDisplayed()
        compose.onNodeWithText("New tool row").assertDoesNotExist()
    }
}
