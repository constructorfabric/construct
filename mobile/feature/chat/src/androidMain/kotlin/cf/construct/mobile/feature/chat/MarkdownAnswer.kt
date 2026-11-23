package cf.construct.mobile.feature.chat

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.ParagraphStyle
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextIndent
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.sp
import cf.construct.mobile.core.designsystem.Spacing
import org.commonmark.node.Block
import org.commonmark.node.Code
import org.commonmark.node.Emphasis
import org.commonmark.node.FencedCodeBlock
import org.commonmark.node.HardLineBreak
import org.commonmark.node.Heading
import org.commonmark.node.HtmlBlock
import org.commonmark.node.HtmlInline
import org.commonmark.node.IndentedCodeBlock
import org.commonmark.node.ListBlock
import org.commonmark.node.Node
import org.commonmark.node.OrderedList
import org.commonmark.node.SoftLineBreak
import org.commonmark.node.StrongEmphasis
import org.commonmark.parser.Parser
import org.commonmark.node.Text as MarkdownText

private val parser = Parser.builder().build()
private val bold = SpanStyle(fontWeight = FontWeight.SemiBold)
private val monospace = SpanStyle(fontFamily = FontFamily.Monospace)

@Composable
internal fun MarkdownAnswer(text: String) {
    val answer = remember(text) { formatAnswer(text) }
    Text(answer, color = MaterialTheme.colorScheme.onBackground, style = MaterialTheme.typography.bodyLarge)
}

internal fun formatAnswer(text: String): AnnotatedString =
    buildAnnotatedString { appendChildren(parser.parse(text), depth = 0) }

private fun Node.children(): Sequence<Node> = generateSequence(firstChild) { it.next }

private fun AnnotatedString.Builder.appendChildren(
    parent: Node,
    depth: Int,
) {
    parent.children().forEach { child ->
        if (child is Block && child.previous != null) {
            append(if (child is ListBlock || child.previous is ListBlock) "\n" else "\n\n")
        }
        appendNode(child, depth)
    }
}

private fun AnnotatedString.Builder.appendNode(
    node: Node,
    depth: Int,
) {
    when (node) {
        is MarkdownText -> append(node.literal)
        is StrongEmphasis, is Heading -> withStyle(bold) { appendChildren(node, depth) }
        is Emphasis -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { appendChildren(node, depth) }
        is Code -> withStyle(monospace) { append(node.literal) }
        is FencedCodeBlock -> withStyle(monospace) { append(node.literal.trimEnd()) }
        is IndentedCodeBlock -> withStyle(monospace) { append(node.literal.trimEnd()) }
        is SoftLineBreak -> append(' ')
        is HardLineBreak -> append('\n')
        is ListBlock -> appendList(node, depth)
        is HtmlBlock -> append(node.literal.trimEnd())
        is HtmlInline -> append(node.literal)
        else -> appendChildren(node, depth)
    }
}

private fun AnnotatedString.Builder.appendList(
    list: ListBlock,
    depth: Int,
    trailingBreak: Boolean = false,
) {
    val start = (list as? OrderedList)?.markerStartNumber
    val indent = Spacing.XL * (depth + 1)
    val style = ParagraphStyle(textIndent = TextIndent(indent.sp, (indent + Spacing.L).sp))
    list.children().forEachIndexed { index, item ->
        item.children().forEachIndexed { block, child ->
            val needsBreak = child.next != null || item.next != null || trailingBreak
            if (child is ListBlock) {
                appendList(child, depth + 1, trailingBreak = needsBreak)
            } else {
                withStyle(style) {
                    if (block == 0) append(if (start == null) "• " else "${start + index}. ")
                    appendNode(child, depth + 1)
                    if (needsBreak) append('\n')
                }
            }
        }
    }
}
