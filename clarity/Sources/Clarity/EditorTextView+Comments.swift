import AppKit
import SwiftUI

/// Comments beside the text: their passages tinted behind the glyphs, and a card for each in the
/// right margin, level with its passage. Cards are subviews, so they scroll with the text; none of
/// it touches the note or its undo stack.
extension EditorTextView {
    static let commentCardWidth: CGFloat = 260
    private static let cardGap: CGFloat = 40

    private var showsComments: Bool { !focusMode && workspace != nil }

    /// Room kept right of the column for the cards, when there are any to show.
    var commentMargin: CGFloat {
        showsComments && !anchoredComments.isEmpty ? Self.commentCardWidth + Self.cardGap : 0
    }

    /// Re-finds every open comment's passage, then repaints passages and repositions cards.
    /// Cheap enough for every keystroke: a handful of string searches.
    func updateComments() {
        guard let workspace, let storage = textStorage else { return }
        let ns = storage.string as NSString
        let margin = commentMargin
        let draft = workspace.draftComment
        anchoredComments = (workspace.comments.filter { !$0.resolved } + [draft].compactMap { $0 })
            .compactMap { comment in comment.range(in: ns).map { (comment, $0) } }
            .sorted { $0.range.location < $1.range.location }
        workspace.commentsDidAnchor(anchoredComments.map(\.comment.id).filter { $0 != draft?.id })

        let underCaret = commentUnderCaret()
        if workspace.activeComment != underCaret {
            workspace.activeComment = underCaret // repaints via didSet
            return
        }
        setNeedsDisplay(visibleRect)
        if commentMargin != margin {
            layoutColumn(force: true)
        } else {
            layoutCommentViews()
        }
    }

    /// Tints commented passages line by line, hugging the glyphs. A background-color attribute
    /// would also fill the indent on every wrapped line, out into the heading gutter.
    func drawCommentHighlights(in rect: NSRect) {
        guard showsComments, let workspace, let layoutManager, let container = textContainer else { return }
        let origin = textContainerOrigin
        let lineSpacing = (styler.fontSize * 0.6).rounded()
        for (comment, range) in anchoredComments {
            let active = comment.id == workspace.activeComment || comment.id == workspace.draftComment?.id
            (active ? Theme.commentActive : Theme.comment).setFill()
            let glyphs = layoutManager.glyphRange(forCharacterRange: range, actualCharacterRange: nil)
            layoutManager.enumerateLineFragments(forGlyphRange: glyphs) { _, _, _, lineGlyphs, _ in
                let part = NSIntersectionRange(glyphs, lineGlyphs)
                guard part.length > 0 else { return }
                var box = layoutManager.boundingRect(forGlyphRange: part, in: container).offsetBy(dx: origin.x, dy: origin.y)
                box.size.height -= lineSpacing
                box = box.insetBy(dx: -3, dy: -1)
                if box.intersects(rect) { NSBezierPath(roundedRect: box, xRadius: 3, yRadius: 3).fill() }
            }
        }
    }

    /// ⌥⌘M: starts a comment on the selection, quoting it with a little context on each side so
    /// the comment can tell this occurrence from any repeats.
    @objc func addComment(_ sender: Any?) {
        let ns = string as NSString
        var range = selectedRange()
        let blank = CharacterSet.whitespacesAndNewlines.union(CharacterSet(charactersIn: "\u{2028}"))
        while range.length > 0, let scalar = Unicode.Scalar(ns.character(at: range.location)), blank.contains(scalar) {
            range = NSRange(location: range.location + 1, length: range.length - 1)
        }
        while range.length > 0, let scalar = Unicode.Scalar(ns.character(at: NSMaxRange(range) - 1)), blank.contains(scalar) {
            range.length -= 1
        }
        guard range.length > 0 else {
            workspace?.flash("Select the text to comment on")
            return
        }
        let context = 32
        let before = ns.rangeOfComposedCharacterSequences(for: NSRange(location: max(0, range.location - context), length: min(context, range.location)))
        let after = ns.rangeOfComposedCharacterSequences(for: NSRange(location: NSMaxRange(range), length: min(context, ns.length - NSMaxRange(range))))
        let anchor = Comment.Anchor(
            exact: ns.substring(with: range),
            prefix: ns.substring(with: NSRange(location: before.location, length: range.location - before.location)),
            suffix: ns.substring(with: NSRange(location: NSMaxRange(range), length: NSMaxRange(after) - NSMaxRange(range)))
        )
        setSelectedRange(NSRange(location: NSMaxRange(range), length: 0))
        workspace?.beginComment(on: anchor)
    }

    func updateActiveComment() {
        guard let workspace else { return }
        let id = commentUnderCaret()
        if workspace.activeComment != id { workspace.activeComment = id }
    }

    private func commentUnderCaret() -> String? {
        let caret = selectedRange()
        return anchoredComments.first { caret.location >= $0.range.location && caret.location <= NSMaxRange($0.range) }?.comment.id
    }

    /// Moves the caret into the comment's passage and scrolls it a third of the way down the window.
    func revealComment(_ id: String) {
        guard let range = anchoredComments.first(where: { $0.comment.id == id })?.range, let scroll = enclosingScrollView else { return }
        setSelectedRange(NSRange(location: range.location, length: 0))
        let line = lineRect(at: range.location)
        let visible = scroll.contentView.bounds
        if line.minY < visible.minY + 60 || line.maxY > visible.maxY - 160 {
            scrollTo(y: line.minY - scroll.contentSize.height / 3)
        }
    }

    // MARK: Cards

    /// Stacks the cards down the margin, each level with its passage unless the one above is in the way.
    func layoutCommentViews() {
        var used = Set<String>()
        if let workspace, showsComments, let container = textContainer {
            let x = columnOriginX + container.containerSize.width + Self.cardGap
            var bottom: CGFloat = 0
            for (comment, range) in anchoredComments {
                let view = comment.id == workspace.draftComment?.id
                    ? commentView(comment.id, DraftCommentCard(
                        author: comment.author,
                        onFinish: { [weak workspace] in workspace?.finishComment($0) },
                        onResize: { [weak self] in self?.layoutCommentViews() }
                    ))
                    : commentView(comment.id, CommentCard(
                        comment: comment,
                        active: comment.id == workspace.activeComment,
                        onResolve: { [weak workspace] in workspace?.resolveComment(comment.id) },
                        onSelect: { [weak workspace] in workspace?.selectComment(comment.id) }
                    ))
                let height = view.fittingSize.height
                let y = max(lineRect(at: range.location).minY - 6, bottom)
                view.frame = NSRect(x: x, y: y, width: Self.commentCardWidth, height: height)
                bottom = y + height + 14
                used.insert(comment.id)
            }
        }
        for (id, view) in commentViews where !used.contains(id) {
            view.removeFromSuperview()
            commentViews[id] = nil
        }
    }

    /// Reuses the comment's view while its kind stays the same, so a draft keeps its typing and focus.
    private func commentView<Card: View>(_ id: String, _ card: Card) -> NSView {
        if let view = commentViews[id] as? NSHostingView<Card> {
            view.rootView = card
            return view
        }
        commentViews[id]?.removeFromSuperview()
        let view = NSHostingView(rootView: card)
        addSubview(view)
        commentViews[id] = view
        return view
    }

    /// The line fragment holding `index`, in view coordinates.
    private func lineRect(at index: Int) -> NSRect {
        guard let layoutManager, let storage = textStorage, storage.length > 0 else { return .zero }
        let glyph = layoutManager.glyphIndexForCharacter(at: min(index, storage.length - 1))
        return layoutManager.lineFragmentUsedRect(forGlyphAt: glyph, effectiveRange: nil)
            .offsetBy(dx: textContainerOrigin.x, dy: textContainerOrigin.y)
    }
}
