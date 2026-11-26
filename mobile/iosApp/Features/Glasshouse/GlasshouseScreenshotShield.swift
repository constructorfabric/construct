import UIKit

/// A field whose canvas layer the system leaves out of screenshots and screen recordings: what is drawn in it shows on
/// the phone and not in the picture. This is not a function Apple documents for the purpose; it is how a secure text
/// field draws, which apps rely on.
@MainActor
protocol SecureCanvasField: AnyObject {
    /// The layer that goes into the layer tree, in the place of what it protects.
    var layer: CALayer { get }
    /// The layer inside it that takes what is to be kept out of screenshots; nil when the system made none.
    var canvasLayer: CALayer? { get }
    /// Gives the field the size of [bounds].
    func fit(to bounds: CGRect)
}

/// The system's secure text field. Only its layers are used: the field is not put into the view hierarchy and takes no
/// touch.
@MainActor
final class SecureTextCanvasField: SecureCanvasField {
    private let field = UITextField()

    init() {
        field.isSecureTextEntry = true
        field.isUserInteractionEnabled = false
        field.backgroundColor = .clear
    }

    var layer: CALayer { field.layer }
    var canvasLayer: CALayer? { field.layer.sublayers?.first }

    func fit(to bounds: CGRect) {
        field.frame = bounds
        field.layoutIfNeeded()
    }
}

/// Keeps one layer out of screenshots and screen recordings by moving it into the canvas layer of a secure field that
/// takes its place under the same parent. The layer is the one that is moved, not a view inside the field: a view put
/// inside a secure field is what is reported to be drawn in screenshots on iOS 18 and later.
///
/// The move either happens completely or changes nothing. It is the layer tree that changes; the layer's view stays
/// where it is in the view hierarchy, so touches, layout and accessibility are those of the view.
@MainActor
final class GlasshouseScreenshotShield {
    /// What was moved and where it came from, kept so that it is put back to the same place whatever the tree looks
    /// like later.
    private struct Move {
        let content: CALayer
        let parent: CALayer
        let index: Int
        let field: SecureCanvasField
        let canvas: CALayer
    }

    /// Moving a layer can drop the drawing that was waiting for it, so its tree is asked to draw again right after
    /// the move, one turn of the run loop later, and after this many seconds.
    static let lateRedraw: TimeInterval = 0.1

    private let makeField: @MainActor () -> SecureCanvasField
    private var move: Move?

    init(makeField: @escaping @MainActor () -> SecureCanvasField = { SecureTextCanvasField() }) {
        self.makeField = makeField
    }

    /// Whether a layer is in the canvas now. A layer that something took out of it since, as removing its view does,
    /// is not.
    var isProtecting: Bool {
        guard let move else { return false }
        return move.content.superlayer === move.canvas
    }

    /// Moves [content] into the canvas of a new secure field. Says whether it is there now: false when the layer has
    /// no parent or the system gave the field no canvas layer, and then the layer tree is as it was. Called again for
    /// the layer that is in the canvas, it changes nothing; called for another layer, or for one that was taken out of
    /// the canvas, it puts the first one back or lets it go first.
    @discardableResult
    func protect(_ content: CALayer) -> Bool {
        if let move {
            if move.content === content && isProtecting { return true }
            restore()
        }
        guard let parent = content.superlayer else { return false }
        let index = parent.sublayers?.firstIndex(of: content) ?? 0
        let field = makeField()
        field.fit(to: parent.bounds)
        parent.insertSublayer(field.layer, at: UInt32(index))
        guard let canvas = field.canvasLayer else {
            field.layer.removeFromSuperlayer()
            return false
        }
        content.removeFromSuperlayer()
        canvas.addSublayer(content)
        move = Move(content: content, parent: parent, index: index, field: field, canvas: canvas)
        redraw(content)
        return true
    }

    /// Takes the field away and puts the moved layer back under the parent it came from, at its place there. A layer
    /// that is no longer in the canvas, because its view was removed or put somewhere else, is left where it is: it is
    /// not this shield's any more. Changes nothing when nothing was moved.
    func restore() {
        guard let move else { return }
        self.move = nil
        let inCanvas = move.content.superlayer === move.canvas
        move.field.layer.removeFromSuperlayer()
        guard inCanvas else { return }
        move.content.removeFromSuperlayer()
        let place = min(move.index, move.parent.sublayers?.count ?? 0)
        move.parent.insertSublayer(move.content, at: UInt32(place))
        redraw(move.content)
    }

    /// The parent changed its size: the field follows it, so the canvas covers what the layer covers.
    func parentResized() {
        guard let move else { return }
        move.field.fit(to: move.parent.bounds)
    }

    private func redraw(_ layer: CALayer) {
        layer.setNeedsDisplayInTree()
        DispatchQueue.main.async { [weak layer] in
            layer?.setNeedsDisplayInTree()
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.lateRedraw) { [weak layer] in
            layer?.setNeedsDisplayInTree()
        }
    }
}

private extension CALayer {
    func setNeedsDisplayInTree() {
        setNeedsDisplay()
        sublayers?.forEach { $0.setNeedsDisplayInTree() }
    }
}
