package it.mauromazzieri.musiccraft

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.path
import androidx.compose.ui.unit.dp

/** The player's buttons, drawn here for MusicCraft. */
object Glyphs {
    private fun glyph(name: String, draw: androidx.compose.ui.graphics.vector.PathBuilder.() -> Unit): ImageVector =
        ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f)
            .path(fill = SolidColor(Color.Black), pathBuilder = draw)
            .build()

    val Play = glyph("Play") {
        moveTo(7f, 4.5f); lineTo(19.5f, 12f); lineTo(7f, 19.5f); close()
    }

    val Pause = glyph("Pause") {
        moveTo(6f, 4.5f); lineTo(10f, 4.5f); lineTo(10f, 19.5f); lineTo(6f, 19.5f); close()
        moveTo(14f, 4.5f); lineTo(18f, 4.5f); lineTo(18f, 19.5f); lineTo(14f, 19.5f); close()
    }

    val Next = glyph("Next") {
        moveTo(5f, 5f); lineTo(15f, 12f); lineTo(5f, 19f); close()
        moveTo(16f, 5f); lineTo(19f, 5f); lineTo(19f, 19f); lineTo(16f, 19f); close()
    }

    val Previous = glyph("Previous") {
        moveTo(19f, 5f); lineTo(9f, 12f); lineTo(19f, 19f); close()
        moveTo(5f, 5f); lineTo(8f, 5f); lineTo(8f, 19f); lineTo(5f, 19f); close()
    }
}
