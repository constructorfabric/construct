package cf.construct.mobile.android

import android.os.Build
import android.util.Log
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import cf.construct.mobile.core.connection.PairResult
import cf.construct.mobile.core.connection.PhoneConnection
import cf.construct.mobile.feature.status.ComputerOperation
import cf.construct.mobile.feature.status.ConnectionHolder
import cf.construct.mobile.feature.status.toOperation
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

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

    /**
     * Every answer the computer can give, and every bad input, is a [PairResult]. The failure message below is
     * for what is left: the phone's own native bridge, whose library did not load (the app logged that at
     * start) or whose call failed.
     */
    private fun pair(action: suspend () -> PairResult) {
        runOperation("Pairing could not be completed. Try again.") { action().toOperation() }
    }

    fun unpair(onRemoved: () -> Unit) {
        runOperation("The computer could not be unpaired. Try again.") {
            try {
                connection.unpair(removeComputerCopy)
            } finally {
                // The copy goes before the pairing is cleared. If clearing fails, the lists must still be read
                // again, or they keep showing a copy that is gone.
                onRemoved()
            }
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
                } catch (cause: Exception) {
                    failed(failure, cause)
                } catch (cause: UnsatisfiedLinkError) {
                    failed(failure, cause)
                }
        }
    }

    private fun failed(
        message: String,
        cause: Throwable,
    ): ComputerOperation {
        // Only the type is logged, so no text of a failure can carry pairing material into the log.
        Log.w(TAG, "$message (${cause.javaClass.name})")
        return ComputerOperation(message = message)
    }

    private companion object {
        const val TAG = "ConnectionViewModel"
    }
}
