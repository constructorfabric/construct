package cf.construct.mobile.feature.status

import cf.construct.mobile.core.connection.PairResult

/** What the pairing and unpairing screens show about the operation in progress or just finished. */
data class ComputerOperation(
    val busy: Boolean = false,
    val paired: Boolean = false,
    val message: String? = null,
    val needsNetworkPermission: Boolean = false,
    val needsFreshCode: Boolean = false,
)

/** The screen's reading of a pairing result. */
fun PairResult.toOperation() =
    ComputerOperation(
        paired = this == PairResult.Paired,
        message = message,
        needsNetworkPermission = this == PairResult.NetworkPermission,
        needsFreshCode = this == PairResult.FreshCode || this == PairResult.Refused,
    )
