package cf.construct.mobile.core.model

import kotlinx.serialization.json.JsonElement

/** The categories of a person's profile, as the gear names them. */
enum class EntityKind { IDENTITY, ROLE, SKILL, PREFERENCE }

/** One fact of the person. */
data class MemoryFact(
    val factId: String,
    val entityKind: EntityKind,
    /** The property by its plain key, such as `skills`. */
    val property: String,
    /** A string, or an object for a property with fields of its own. */
    val value: JsonElement,
    /** When the fact was stored (RFC 3339). */
    val storedAt: String,
    /** The tool the fact came from; the gear does not return it yet. */
    val tool: String? = null,
)

/** Every fact of the person, with the profile's version (null for a person without a profile). */
data class MemoryFacts(
    val facts: List<MemoryFact>,
    val version: Long?,
)

/** The answer to "delete everything"; [running] is true when an erasure was already on. */
data class EraseStarted(
    val running: Boolean,
)
