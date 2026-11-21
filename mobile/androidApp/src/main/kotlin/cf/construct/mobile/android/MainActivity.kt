package cf.construct.mobile.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import cf.construct.mobile.core.construct.Construct
import cf.construct.mobile.feature.memory.MemoryHolder
import cf.construct.mobile.feature.memory.MemoryScreen

class MainActivity : ComponentActivity() {
    private val memory: MemoryViewModel by viewModels {
        viewModelFactory { initializer { MemoryViewModel((application as ConstructApp).construct) } }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        setContent {
            ConstructTheme {
                val state by memory.holder.state.collectAsState()
                Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    MemoryScreen(state, onRefresh = memory.holder::refresh, modifier = Modifier.safeDrawingPadding())
                }
            }
        }
    }
}

/** Keeps the Memory holder across a turn of the screen. */
internal class MemoryViewModel(
    construct: Construct,
) : ViewModel() {
    val holder = MemoryHolder(construct, viewModelScope)
}
