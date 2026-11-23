package cf.construct.mobile.core.designsystem

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.dp

private const val VIEWPORT = 24f
private const val STROKE = 2f

/** Builds a stroked vector from Lucide (ISC licence) paths. The caller supplies the tint. */
fun lucide(
    name: String,
    vararg paths: String,
): ImageVector =
    ImageVector
        .Builder(name, VIEWPORT.dp, VIEWPORT.dp, VIEWPORT, VIEWPORT)
        .apply {
            for (data in paths) {
                addPath(
                    pathData = PathParser().parsePathString(data).toNodes(),
                    stroke = SolidColor(Color.Black),
                    strokeLineWidth = STROKE,
                    strokeLineCap = StrokeCap.Round,
                    strokeLineJoin = StrokeJoin.Round,
                )
            }
        }.build()
