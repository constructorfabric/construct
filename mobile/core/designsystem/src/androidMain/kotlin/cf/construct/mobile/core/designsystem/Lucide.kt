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

/** The circle that the Lucide icons "info", "circle-alert", "circle-check" and "circle" start from. */
const val LUCIDE_CIRCLE = "M22 12a10 10 0 1 1-20 0a10 10 0 1 1 20 0"

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
