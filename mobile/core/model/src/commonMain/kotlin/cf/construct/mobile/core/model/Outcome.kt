package cf.construct.mobile.core.model

/** What a call of the Construct interface gives back: a value, or the reason it failed. */
sealed interface Outcome<out T> {
    data class Ok<out T>(
        val value: T,
    ) : Outcome<T>

    data class Failed(
        val error: String,
    ) : Outcome<Nothing>
}

/** The value of a successful outcome, or null. */
fun <T> Outcome<T>.valueOrNull(): T? = (this as? Outcome.Ok)?.value
