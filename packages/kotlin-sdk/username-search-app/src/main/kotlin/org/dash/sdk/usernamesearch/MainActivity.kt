package org.dash.sdk.usernamesearch

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface

/**
 * Single-activity, single-screen Compose app. The whole UI is [UsernameSearchScreen];
 * there are no XML layouts or fragments.
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                Surface {
                    UsernameSearchScreen()
                }
            }
        }
    }
}
