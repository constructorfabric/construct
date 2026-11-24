package cf.construct.mobile.android

import android.os.Build
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import cf.construct.mobile.core.connection.PairResult
import cf.construct.mobile.core.connection.PhoneConnection
import cf.construct.mobile.feature.status.ConnectionHolder
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

internal data class ComputerOperation(
    val busy: Boolean = false,
    val paired: Boolean = false,
    val message: String? = null,
    val needsNetworkPermission: Boolean = false,
    val needsFreshCode: Boolean = false,
)

internal class ConnectionViewModel(
    private val connection: PhoneConnection,
    private val removeComputerCopy: () -> Unit,
) : ViewModel() {
    val holder = ConnectionHolder(connection, viewModelScope)
    private val operationValue = MutableStateFlow(ComputerOperation())
    val operation = operationValue.asStateFlow()

    fun pairByCode(
        address: String,
        code: String,
    ) = pair { connection.pairByCode(address, code, Build.MODEL) }

    fun pairByQr(payload: String) = pair { connection.pairByQr(payload, Build.MODEL) }

    private fun pair(action: suspend () -> PairResult) {
        runOperation("Pairing could not be completed. Try again.") {
            val result = action()
            ComputerOperation(
                paired = result == PairResult.Paired,
                message = result.message,
                needsNetworkPermission = result == PairResult.NetworkPermission,
                needsFreshCode = result == PairResult.FreshCode || result == PairResult.Refused,
            )
        }
    }

    fun unpair(onRemoved: () -> Unit) {
        runOperation("The computer could not be unpaired. Try again.") {
            connection.unpair(removeComputerCopy)
            onRemoved()
            ComputerOperation()
        }
    }

    fun clearOperation() {
        if (!operationValue.value.busy) operationValue.value = ComputerOperation()
    }

    @Suppress("TooGenericExceptionCaught")
    private fun runOperation(
        failure: String,
        action: suspend () -> ComputerOperation,
    ) {
        if (operationValue.value.busy) return
        operationValue.value = ComputerOperation(busy = true)
        viewModelScope.launch {
            operationValue.value =
                try {
                    action()
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (_: Exception) {
                    ComputerOperation(message = failure)
                } catch (_: UnsatisfiedLinkError) {
                    ComputerOperation(message = failure)
                }
        }
    }
}
