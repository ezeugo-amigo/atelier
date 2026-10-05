import AppKit

/// The writing surface: a plain-text NSTextView that restyles Markdown on every edit, keeps a
/// fixed-width column centered in the window, and in focus mode dims everything but the current
/// sentence while keeping the caret line vertically centered.
final class EditorTextView: NSTextView, NSTextStorageDelegate {
    /// Space above the first line and below the last, outside focus mode.
    static let verticalInset: CGFloat = 72

    weak var workspace: Workspace?

    /// Width of the writing column, in characters.
    var columnCharacters = 80 {
        didSet { layoutColumn(force: true) }
    }

    var fontSize: CGFloat = 16 {
        didSet {
            guard fontSize != oldValue else { return }
            styler = MarkdownStyler(fontSize: fontSize)
            restyle()
            layoutColumn(force: true)
        }
    }

    var focusMode = false {
        didSet {
            updateComments()
            layoutColumn(force: true)
            updateFocusDimming()
            centerCaret()
        }
    }

    private(set) var styler = MarkdownStyler(fontSize: 16)
    /// Fences, frontmatter and comments as of the last restyle; see `MarkdownStyler.apply`.
    private var structureSignature = ""
    /// A text view built around an existing container doesn't retain its storage.
    private var ownedStorage: NSTextStorage?
    private(set) var columnOriginX: CGFloat = 0
    /// Open comments whose passage was found, in document order; see `updateComments`.
    var anchoredComments: [(comment: Comment, range: NSRange)] = []
    /// Cards and markers drawn over the text, by comment id and role.
    var commentViews: [String: NSView] = [:]
    private var laidOutSize: NSSize = .zero
    /// Opening a note isn't an edit, so it stays out of the history.
    private var isLoading = false

    static func makeScrollable(fontSize: CGFloat) -> (NSScrollView, EditorTextView) {
        let storage = NSTextStorage()
        let layoutManager = NSLayoutManager()
        // Estimated (non-contiguous) layout re-guesses the height of off-screen text whenever it's
        // invalidated, which moves the page after any full restyle. Exact layout keeps it still.
        layoutManager.allowsNonContiguousLayout = false
        storage.addLayoutManager(layoutManager)
        let container = NSTextContainer(size: NSSize(width: 600, height: CGFloat.greatestFiniteMagnitude))
        container.widthTracksTextView = false
        container.lineFragmentPadding = 0
        layoutManager.addTextContainer(container)

        let editor = EditorTextView(frame: .zero, textContainer: container)
        editor.ownedStorage = storage
        storage.delegate = editor
        editor.configure()
        editor.fontSize = fontSize

        let scroll = NSScrollView()
        scroll.documentView = editor
        scroll.drawsBackground = false
        scroll.borderType = .noBorder
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.scrollerStyle = .overlay
        // The editor pads itself (textContainerInset). Automatic insets for the transparent title
        // bar make AppKit re-clamp the scroll position whenever the text grows, jolting the page.
        scroll.automaticallyAdjustsContentInsets = false
        scroll.contentInsets = NSEdgeInsetsZero
        scroll.contentView.postsFrameChangedNotifications = true
        NotificationCenter.default.addObserver(
            editor, selector: #selector(viewportResized), name: NSView.frameDidChangeNotification, object: scroll.contentView
        )
        return (scroll, editor)
    }

    private func configure() {
        drawsBackground = false
        isRichText = false
        importsGraphics = false
        allowsUndo = true
        usesFindBar = true
        isIncrementalSearchingEnabled = true
        isAutomaticQuoteSubstitutionEnabled = false
        isAutomaticDashSubstitutionEnabled = false
        isAutomaticTextReplacementEnabled = false
        isAutomaticSpellingCorrectionEnabled = false
        isAutomaticLinkDetectionEnabled = false
        isContinuousSpellCheckingEnabled = false
        smartInsertDeleteEnabled = false
        inlinePredictionType = .no
        if #available(macOS 15.0, *) { mathExpressionCompletionType = .no }
        insertionPointColor = Theme.accent
        selectedTextAttributes = [.backgroundColor: Theme.selection]
        isVerticallyResizable = true
        isHorizontallyResizable = false
        autoresizingMask = [.width]
        minSize = .zero
        maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
    }

    // MARK: Content

    func load(_ text: String, keepSelection: Bool = false) {
        isLoading = true
        defer { isLoading = false }
        let selection = selectedRange()
        string = text
        typingAttributes = styler.baseAttributes
        undoManager?.removeAllActions()
        if keepSelection {
            setSelectedRange(NSRange(location: min(selection.location, (text as NSString).length), length: 0))
        } else {
            setSelectedRange(NSRange(location: 0, length: 0))
            scroll(.zero)
        }
        updateFocusDimming()
        updateComments()
        centerCaret()
    }

    private func restyle() {
        guard let storage = textStorage else { return }
        storage.beginEditing()
        structureSignature = styler.apply(to: storage)
        storage.endEditing()
        typingAttributes = styler.baseAttributes
    }

    /// Restyling in willProcessEditing would widen the edited range to the whole document and throw
    /// the caret to the end, so style afterwards and redo AppKit's font fallback (for ⌘, emoji, CJK)
    /// that our attributes just replaced.
    ///
    /// Also the one place every character change passes, undo and redo included (they bypass
    /// shouldChangeText), so it's where edits are recorded for the note's history.
    func textStorage(
        _ textStorage: NSTextStorage, didProcessEditing editedMask: NSTextStorageEditActions, range editedRange: NSRange, changeInLength delta: Int
    ) {
        guard editedMask.contains(.editedCharacters) else { return }
        if !isLoading { workspace?.textDidEdit(editedRange, changeInLength: delta, in: textStorage.mutableString) }
        let ns = textStorage.string as NSString
        var dirty = ns.paragraphRange(for: editedRange)
        if dirty.location > 0 {
            dirty = NSUnionRange(dirty, ns.paragraphRange(for: NSRange(location: dirty.location - 1, length: 0)))
        }
        if NSMaxRange(dirty) < ns.length {
            dirty = NSUnionRange(dirty, ns.paragraphRange(for: NSRange(location: NSMaxRange(dirty), length: 0)))
        }
        let signature = styler.apply(to: textStorage, in: dirty)
        if signature != structureSignature {
            dirty = NSRange(location: 0, length: ns.length)
            styler.apply(to: textStorage)
            structureSignature = signature
        }
        textStorage.fixAttributes(in: dirty)
    }

    override func didChangeText() {
        super.didChangeText()
        workspace?.textDidChange(string)
        updateComments()
        // Typing moves the caret without a usable selection-change callback, so recenter here too;
        // otherwise the caret drifts a line per wrap and the view later jumps to catch up.
        if focusMode {
            updateFocusDimming()
            centerCaret()
        } else {
            keepCaretAboveBottom()
        }
    }

    // MARK: Layout

    override func drawBackground(in rect: NSRect) {
        super.drawBackground(in: rect)
        drawCommentHighlights(in: rect)
    }

    override var textContainerOrigin: NSPoint {
        NSPoint(x: columnOriginX, y: textContainerInset.height)
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        layoutColumn()
    }

    @objc private func viewportResized() {
        layoutColumn()
    }

    /// Centers a column of `columnCharacters`. The text container also spans the gutter to its left,
    /// where heading markers hang. When the window is too narrow to center it, the column takes
    /// all the room between the margins and shifts right, since only the left side needs a gutter.
    func layoutColumn(force: Bool = false) {
        let visibleHeight = enclosingScrollView?.contentSize.height ?? frame.height
        let size = NSSize(width: frame.width, height: visibleHeight)
        guard force || size != laidOutSize else { return }
        laidOutSize = size

        let gutter = styler.gutter
        let margin: CGFloat = 24
        let reserve = commentMargin
        let column = max(min(styler.charWidth * CGFloat(columnCharacters), frame.width - 2 * margin - gutter - reserve), styler.charWidth * 20)
        textContainer?.containerSize = NSSize(width: column + gutter, height: CGFloat.greatestFiniteMagnitude)
        columnOriginX = max(margin, ((frame.width - column - reserve) / 2 - gutter).rounded())
        textContainerInset = NSSize(width: 0, height: focusMode ? (visibleHeight / 2).rounded() : Self.verticalInset)
        invalidateTextContainerOrigin()
        layoutCommentViews()
        needsDisplay = true
    }

    // MARK: Focus mode

    override func setSelectedRanges(_ ranges: [NSValue], affinity: NSSelectionAffinity, stillSelecting: Bool) {
        super.setSelectedRanges(ranges, affinity: affinity, stillSelecting: stillSelecting)
        if !stillSelecting, !isLoading, let range = ranges.first?.rangeValue { workspace?.selectionDidChange(range) }
        if !stillSelecting { updateActiveComment() }
        guard focusMode else { return }
        updateFocusDimming()
        if !stillSelecting { centerCaret() }
    }

    private func updateFocusDimming() {
        guard let layoutManager, let storage = textStorage else { return }
        let length = storage.length
        layoutManager.removeTemporaryAttribute(.foregroundColor, forCharacterRange: NSRange(location: 0, length: length))
        guard focusMode else { return }

        let ns = storage.string as NSString
        let selection = selectedRange()
        let caret = min(selection.location, length)
        var focus = ns.paragraphRange(for: NSRange(location: caret, length: min(selection.length, length - caret)))
        if selection.length == 0 {
            ns.enumerateSubstrings(in: focus, options: [.bySentences, .substringNotRequired]) { _, sentence, enclosing, stop in
                if caret >= sentence.location && caret <= NSMaxRange(enclosing) {
                    focus = sentence
                    stop.pointee = true
                }
            }
        }
        layoutManager.addTemporaryAttribute(.foregroundColor, value: Theme.dimmed, forCharacterRange: NSRange(location: 0, length: focus.location))
        layoutManager.addTemporaryAttribute(
            .foregroundColor, value: Theme.dimmed, forCharacterRange: NSRange(location: NSMaxRange(focus), length: length - NSMaxRange(focus))
        )
    }

    /// The caret's line, in view coordinates.
    private func caretLineRect() -> NSRect? {
        guard let layoutManager, let storage = textStorage else { return nil }
        let location = selectedRange().location
        var rect: NSRect
        if location >= storage.length && !layoutManager.extraLineFragmentRect.isEmpty {
            rect = layoutManager.extraLineFragmentRect
        } else if storage.length > 0 {
            let glyph = layoutManager.glyphIndexForCharacter(at: min(location, storage.length - 1))
            rect = layoutManager.lineFragmentUsedRect(forGlyphAt: glyph, effectiveRange: nil)
        } else {
            return nil
        }
        rect.origin.y += textContainerOrigin.y
        return rect
    }

    func scrollTo(y: CGFloat) {
        guard let scroll = enclosingScrollView else { return }
        scroll.contentView.scroll(to: NSPoint(x: 0, y: max(0, y.rounded())))
        scroll.reflectScrolledClipView(scroll.contentView)
    }

    /// Typewriter scrolling: keep the caret's line in the middle of the window.
    private func centerCaret() {
        guard focusMode, let line = caretLineRect(), let scroll = enclosingScrollView else { return }
        scrollTo(y: line.midY - scroll.contentSize.height / 2)
    }

    /// While typing, keeps the caret's line a margin's height above the bottom of the window,
    /// scrolling exactly as far as each new line needs. Left to itself, AppKit lets the caret sink
    /// into the bottom padding and then jumps several lines at once.
    private func keepCaretAboveBottom() {
        guard let line = caretLineRect(), let visible = enclosingScrollView?.contentView.bounds else { return }
        let overflow = line.maxY - (visible.maxY - Self.verticalInset)
        if overflow > 0 { scrollTo(y: visible.origin.y + overflow) }
    }

    /// Copies cell line breaks as `<br>`, the way they're saved.
    override func writeSelection(to pboard: NSPasteboard, type: NSPasteboard.PasteboardType) -> Bool {
        guard type == .string else { return super.writeSelection(to: pboard, type: type) }
        let text = selectedRanges.map { (string as NSString).substring(with: $0.rangeValue) }.joined(separator: "\n")
        return pboard.setString(MarkdownTable.diskText(fromEditor: text), forType: .string)
    }

    /// Copies the selection, or the whole note if nothing is selected, as HTML for pasting into
    /// rich-text apps like Notion, with the Markdown alongside for plain-text apps.
    @objc func copyAsRichText(_ sender: Any?) {
        let ns = string as NSString
        let ranges = selectedRange().length > 0 ? selectedRanges.map(\.rangeValue) : [NSRange(location: 0, length: ns.length)]
        let markdown = MarkdownTable.diskText(fromEditor: ranges.map(ns.substring).joined(separator: "\n"))
        let pboard = NSPasteboard.general
        pboard.clearContents()
        pboard.setString(MarkdownHTML.render(markdown), forType: .html)
        pboard.setString(markdown, forType: .string)
        workspace?.flash("\(markdown.count.formatted()) characters copied as rich text")
    }

    // MARK: Input

    override func keyDown(with event: NSEvent) {
        NSCursor.setHiddenUntilMouseMoves(true)
        super.keyDown(with: event)
    }

    override func mouseDown(with event: NSEvent) {
        if event.modifierFlags.contains(.command) {
            let index = characterIndexForInsertion(at: convert(event.locationInWindow, from: nil))
            if let target = linkTarget(near: index) {
                workspace?.follow(target)
                return
            }
        }
        super.mouseDown(with: event)
    }

    /// Continues lists, task lists and blockquotes; Return on an empty item ends the list.
    override func insertNewline(_ sender: Any?) {
        if expandSlashCommand() || handleTableNewline() { return }
        let ns = string as NSString
        let selection = selectedRange()
        let line = ns.paragraphRange(for: NSRange(location: selection.location, length: 0))
        let head = NSRange(location: line.location, length: selection.location - line.location)
        let before = ns.substring(with: head)
        let local = NSRange(location: 0, length: head.length)

        var prefix: String?
        var next: String?
        if let m = MarkdownStyler.listItem.firstMatch(in: before, range: local) {
            let part = { (i: Int) in m.range(at: i).location == NSNotFound ? "" : (before as NSString).substring(with: m.range(at: i)) }
            var marker = part(2)
            if let number = Int(marker.dropLast()) { marker = "\(number + 1)\(marker.last!)" }
            prefix = (before as NSString).substring(with: m.range)
            next = part(1) + marker + part(3) + (part(4).isEmpty ? "" : "[ ] ")
        } else if let m = MarkdownStyler.quote.firstMatch(in: before, range: local) {
            prefix = (before as NSString).substring(with: m.range)
            next = prefix
        }

        guard selection.length == 0, let prefix, let next else {
            super.insertNewline(sender)
            return
        }
        let restOfLine = ns.substring(with: NSRange(location: selection.location, length: NSMaxRange(line) - selection.location))
        if before == prefix && restOfLine.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            insertText("", replacementRange: head)
        } else {
            insertText("\n" + next, replacementRange: selection)
        }
    }

    override func insertTab(_ sender: Any?) {
        if expandSlashCommand() || moveTableCell(by: 1) { return }
        guard let line = currentListLine() else { return super.insertTab(sender) }
        replace(NSRange(location: line.location, length: 0), with: "\t")
    }

    override func insertBacktab(_ sender: Any?) {
        if moveTableCell(by: -1) { return }
        guard let line = currentListLine() else { return super.insertBacktab(sender) }
        let leading = (string as NSString).substring(with: line).prefix { $0 == "\t" || $0 == " " }
        let width = leading.first == "\t" ? 1 : min(leading.count, 4)
        if width > 0 { replace(NSRange(location: line.location, length: width), with: "") }
    }

    private func currentListLine() -> NSRange? {
        let ns = string as NSString
        let line = ns.paragraphRange(for: NSRange(location: selectedRange().location, length: 0))
        let text = ns.substring(with: line)
        return MarkdownStyler.listItem.firstMatch(in: text, range: NSRange(location: 0, length: (text as NSString).length)) == nil ? nil : line
    }

    /// Replaces text without moving the caret relative to the surrounding words.
    private func replace(_ range: NSRange, with text: String) {
        let selection = selectedRange()
        let delta = (text as NSString).length - range.length
        insertText(text, replacementRange: range)
        let location = selection.location >= NSMaxRange(range) ? selection.location + delta : selection.location
        setSelectedRange(NSRange(location: max(0, location), length: selection.length))
    }

    // MARK: Formatting commands (Format menu, via the responder chain)

    @objc func formatBold(_ sender: Any?) { wrapSelection(with: "**") }
    @objc func formatItalic(_ sender: Any?) { wrapSelection(with: "*") }
    @objc func formatStrikethrough(_ sender: Any?) { wrapSelection(with: "~~") }
    @objc func formatHighlight(_ sender: Any?) { wrapSelection(with: "==") }
    @objc func formatCode(_ sender: Any?) { wrapSelection(with: "`") }

    @objc func formatLink(_ sender: Any?) {
        let selection = selectedRange()
        let selected = (string as NSString).substring(with: selection)
        if selected.isEmpty {
            insertText("[[]]", replacementRange: selection)
            setSelectedRange(NSRange(location: selection.location + 2, length: 0))
        } else {
            insertText("[\(selected)]()", replacementRange: selection)
            setSelectedRange(NSRange(location: selection.location + selection.length + 3, length: 0))
        }
    }

    @objc func toggleTask(_ sender: Any?) {
        let ns = string as NSString
        let line = ns.paragraphRange(for: NSRange(location: selectedRange().location, length: 0))
        let text = ns.substring(with: line)
        guard let m = MarkdownStyler.listItem.firstMatch(in: text, range: NSRange(location: 0, length: (text as NSString).length)) else {
            let indent = text.prefix { $0 == " " || $0 == "\t" }.utf16.count
            replace(NSRange(location: line.location + indent, length: 0), with: "- [ ] ")
            return
        }
        let box = m.range(at: 4)
        if box.location == NSNotFound {
            replace(NSRange(location: line.location + NSMaxRange(m.range), length: 0), with: "[ ] ")
        } else {
            let mark = NSRange(location: line.location + box.location + 1, length: 1)
            replace(mark, with: ns.substring(with: mark) == " " ? "x" : " ")
        }
    }

    /// ⌘↩: a line break inside a table cell (links there still open with ⌘-click), otherwise
    /// follows the link under the caret.
    @objc func followLink(_ sender: Any?) {
        if insertCellLineBreak() { return }
        if let target = linkTarget(near: selectedRange().location) { workspace?.follow(target) }
    }

    /// Wraps the selection in `marker`, or unwraps it if it's already wrapped.
    private func wrapSelection(with marker: String) {
        let ns = string as NSString
        let m = (marker as NSString).length
        let selection = selectedRange()
        let selected = ns.substring(with: selection)
        let outer = NSRange(location: selection.location - m, length: selection.length + 2 * m)
        if selection.location >= m, NSMaxRange(outer) <= ns.length,
           ns.substring(with: NSRange(location: outer.location, length: m)) == marker,
           ns.substring(with: NSRange(location: NSMaxRange(selection), length: m)) == marker {
            insertText(selected, replacementRange: outer)
            setSelectedRange(NSRange(location: outer.location, length: selection.length))
        } else {
            insertText(marker + selected + marker, replacementRange: selection)
            setSelectedRange(NSRange(location: selection.location + m, length: selection.length))
        }
    }

    private func linkTarget(near index: Int) -> String? {
        guard let storage = textStorage else { return nil }
        for i in [index, index - 1] where i >= 0 && i < storage.length {
            if let target = storage.attribute(.clarityLink, at: i, effectiveRange: nil) as? String { return target }
        }
        return nil
    }
}
