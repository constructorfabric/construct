package cf.construct.mobile.core.chat

/** What a fake provider does with one request. */
sealed interface Reply {
    /** Answers with [code] and the JSON text [body]. */
    data class Status(
        val code: Int,
        val body: String,
    ) : Reply

    /** Accepts the request and never answers. */
    data object Hang : Reply
}

/** A provider on a local port that answers the requests it gets with [Reply]s, in order. */
interface FakeProvider {
    /** The address to give as the model address, such as `http://127.0.0.1:50321`. */
    val baseUrl: String

    /** The bodies of the requests received so far. */
    val bodies: List<String>

    fun close()
}

/** What each platform gives the tests: a fake provider and an address where nothing listens. */
interface Fakes {
    /** Starts a fake provider on a free local port. */
    fun start(replies: List<Reply>): FakeProvider

    /** A local address where nothing listens. */
    fun closedAddress(): String
}
