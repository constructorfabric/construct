package cf.construct.mobile.core.database

import cf.construct.mobile.core.model.ChatMessage
import cf.construct.mobile.core.model.DashboardDocument
import cf.construct.mobile.core.model.DashboardInput
import cf.construct.mobile.core.model.DashboardNumbers
import cf.construct.mobile.core.model.MemoryFact

/**
 * What the copy remembers about where it came from and how far it has been brought up to date. A copy that
 * never synced has no computer, no marker and no tokens.
 */
data class SyncRecord(
    /** The fingerprint of the certificate of the computer the copy came from. */
    val computer: String? = null,
    /** The marker of the computer's last erasure that the copy has seen. */
    val resetMarker: String? = null,
    /** The token of each part as the computer gave it when that part was last copied. */
    val factsToken: String? = null,
    val chatsToken: String? = null,
    val dashboardToken: String? = null,
    /** The time zone of the computer's days. */
    val zone: String? = null,
    /** Whether one whole sync from [computer] has been applied. */
    val firstCopyDone: Boolean = false,
    /** When the copy was last brought up to date (RFC 3339). */
    val syncedAt: String? = null,
)

/** A chat of the computer as the copy holds it: how many lines it has decides what is still to be fetched. */
data class ChatOnCopy(
    val id: String,
    val title: String,
    val items: Int,
)

/**
 * A chat of the computer. [messages] are the lines from position [from] on: with 0, they are the whole chat and
 * replace what the copy holds; with more, they continue what the copy holds, which must be exactly [from] lines.
 * A null [time] keeps the time the copy holds.
 */
class ChatUpdate(
    val id: String,
    val title: String,
    val time: String?,
    val from: Int,
    val messages: List<ChatMessage>,
)

class FactsUpdate(
    val token: String,
    val facts: List<MemoryFact>,
)

/** Every chat of the computer, in the order it lists them: a chat of the copy that is not in it is removed. */
class ChatsUpdate(
    val token: String,
    val chats: List<ChatUpdate>,
)

/**
 * The dashboard of the computer: its document (none when it has none), the zone its days are in, and every
 * numbers answer it gave under the input it was counted for. What the copy held of the numbers is replaced.
 */
class DashboardUpdate(
    val token: String,
    val zone: String,
    val document: DashboardDocument?,
    val numbers: List<Pair<DashboardInput, DashboardNumbers>>,
)

/**
 * One sync, whole. A part that is null did not change and is not touched. [computer] is the identity of the
 * computer it came from: the copy refuses it unless it is the copy's own computer. [resetMarker] is the marker
 * the computer gave in this sync.
 */
class CopyUpdate(
    val computer: String,
    val syncedAt: String,
    val resetMarker: String?,
    val facts: FactsUpdate? = null,
    val chats: ChatsUpdate? = null,
    val dashboard: DashboardUpdate? = null,
)

/** The copy is not what the update was made for: another computer was paired meanwhile, or two syncs ran at once. */
class CopyChanged : Exception("The copy changed while the sync ran.")
