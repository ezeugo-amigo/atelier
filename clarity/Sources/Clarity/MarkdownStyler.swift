import AppKit

extension NSAttributedString.Key {
    /// The raw target of a wikilink or Markdown link, followed with ⌘-click or ⌘↩.
    static let clarityLink = NSAttributedString.Key("ClarityLink")
}

/// Styles Markdown in place, iA Writer style: syntax characters kept visible but faint, and heading
/// hashes hung in the left margin so titles align with body text. Headings scale by level.
/// Covers GitHub-flavored Markdown plus Obsidian's wikilinks, tags, callouts, highlights and comments.
struct MarkdownStyler {
    let fontSize: CGFloat
    let charWidth: CGFloat
    let baseAttributes: [NSAttributedString.Key: Any]

    /// Room left of the text column for hanging heading markers ("###### ").
    var gutter: CGFloat { charWidth * 7 }

    /// Heading sizes relative to body text, for `#` through `######`.
    static let headingScale: [CGFloat] = [1.6, 1.35, 1.15, 1, 1, 1]

    init(fontSize: CGFloat) {
        let font = Theme.font(size: fontSize)
        self.fontSize = fontSize
        charWidth = ("M" as NSString).size(withAttributes: [.font: font]).width
        baseAttributes = [
            .font: font,
            .foregroundColor: Theme.text,
            .paragraphStyle: Self.paragraph(fontSize: fontSize, charWidth: charWidth, first: charWidth * 7, rest: charWidth * 7),
        ]
    }

    private static func paragraph(fontSize: CGFloat, charWidth: CGFloat, first: CGFloat, rest: CGFloat) -> NSParagraphStyle {
        let style = NSMutableParagraphStyle()
        style.firstLineHeadIndent = first
        style.headIndent = rest
        style.lineSpacing = (fontSize * 0.6).rounded()
        style.defaultTabInterval = charWidth * 4
        style.tabStops = []
        return style
    }

    private func paragraph(first: CGFloat, rest: CGFloat) -> NSParagraphStyle {
        Self.paragraph(fontSize: fontSize, charWidth: charWidth, first: first, rest: rest)
    }

    // MARK: Patterns

    private static func regex(_ pattern: String, _ options: NSRegularExpression.Options = []) -> NSRegularExpression {
        try! NSRegularExpression(pattern: pattern, options: options)
    }

    static let fenceOpen = regex(#"^ {0,3}(`{3,}|~{3,})"#)
    static let heading = regex(#"^(#{1,6} +)"#)
    static let rule = regex(#"^ {0,3}([-*_])( *\1){2,} *$"#)
    static let quote = regex(#"^( {0,3}>[> ]*)"#)
    static let callout = regex(#"^\[![^\]]+\][+-]?"#)
    static let listItem = regex(#"^([ \t]*)([-*+]|\d{1,9}[.)])([ \t]+)(\[[ xX]\](?:[ \t]+|$))?"#)
    static let tableSeparator = regex(#"^ *\|? *:?-+:? *(\| *:?-+:? *)*\|? *$"#)
    static let pipe = regex(#"\|"#)
    static let footnoteDefinition = regex(#"^\[\^[^\]]+\]:"#)

    static let inlineCode = regex(#"(`+)(.+?)(\1)"#)
    static let wikilink = regex(#"(!?\[\[)([^\[\]\n|]+?)(\|[^\[\]\n]*)?(\]\])"#)
    static let link = regex(#"(!?\[)([^\]\n]*)(\]\()([^)\s]*)((?:\s+"[^"\n]*")?\))"#)
    static let bareURL = regex(#"<?https?://[^\s<>)\]]+>?"#)
    static let footnoteRef = regex(#"\[\^[^\]\s]+\]"#)
    static let lineBreak = regex(#"<br\s*/?>"#, .caseInsensitive)
    static let bold = regex(#"(\*\*|__)(?=\S)(.+?)(?<=\S)(\1)"#)
    static let italicStar = regex(#"(?<![*\\])(\*)(?=[^\s*])(.+?)(?<=[^\s*\\])(\*)(?!\*)"#)
    static let italicUnderscore = regex(#"(?<![\w\\])(_)(?=[^\s_])(.+?)(?<=[^\s_\\])(_)(?!\w)"#)
    static let strike = regex(#"(~~)(?=\S)(.+?)(?<=\S)(~~)"#)
    static let mark = regex(#"(==)(?=\S)(.+?)(?<=\S)(==)"#)
    static let tag = regex(#"(?<![\w#&/\]])#[\p{L}_][\p{L}\p{N}_/-]*"#)
    static let comment = regex(#"%%[\s\S]*?%%|<!--[\s\S]*?-->"#)

    // MARK: Styling

    /// Styles the paragraphs within `dirty` (the whole document if nil). Every line is still scanned,
    /// because fences and frontmatter change how later lines look; the returned signature describes
    /// that structure, and when it changes the caller should restyle everything.
    ///
    /// Restyling only what changed matters beyond speed: touching attributes everywhere makes the
    /// layout manager re-estimate the height of off-screen text, which shifts the view while typing.
    @discardableResult
    func apply(to storage: NSTextStorage, in dirty: NSRange? = nil) -> String {
        let text = storage.string
        let ns = text as NSString
        let target = dirty ?? NSRange(location: 0, length: ns.length)
        func touches(_ line: NSRange) -> Bool {
            NSIntersectionRange(line, target).length > 0 || line.location == target.location
        }
        storage.setAttributes(baseAttributes, range: target)

        var signature = ""
        var fence: String?
        var inFrontmatter = false
        var location = 0
        while location < ns.length {
            let isFirstLine = location == 0
            let full = ns.paragraphRange(for: NSRange(location: location, length: 0))
            location = NSMaxRange(full)
            var end = NSMaxRange(full)
            while end > full.location, [10, 13].contains(ns.character(at: end - 1)) { end -= 1 }
            let range = NSRange(location: full.location, length: end - full.location)
            let line = ns.substring(with: range)
            let styled = touches(full)

            if isFirstLine && line == "---" {
                inFrontmatter = true
                signature += "<fm>"
                if styled { faint(range, storage) }
            } else if inFrontmatter {
                inFrontmatter = !(line == "---" || line == "...")
                if !inFrontmatter { signature += "</fm>" }
                if styled { color(inFrontmatter ? Theme.muted : Theme.faint, range, storage) }
            } else if let marker = fence {
                let trimmed = line.trimmingCharacters(in: .whitespaces)
                let closes = trimmed.count >= marker.count && trimmed.allSatisfy({ $0 == marker.first })
                if closes {
                    fence = nil
                    signature += "</\(marker)>"
                }
                if styled { closes ? faint(range, storage) : color(Theme.code, range, storage) }
            } else if let match = Self.fenceOpen.firstMatch(in: line, range: NSRange(location: 0, length: range.length)) {
                let marker = (line as NSString).substring(with: match.range(at: 1))
                fence = marker
                signature += "<\(marker)>"
                if styled { faint(range, storage) }
            } else if styled {
                styleBlock(line, at: range, text: text, in: storage)
            }
        }

        for match in Self.comment.matches(in: text, range: NSRange(location: 0, length: ns.length)) {
            signature += "<!>"
            let overlap = NSIntersectionRange(match.range, target)
            if overlap.length > 0 { faint(overlap, storage) }
        }
        return signature
    }

    private func styleBlock(_ line: String, at range: NSRange, text: String, in storage: NSTextStorage) {
        let local = NSRange(location: 0, length: range.length)
        func global(_ r: NSRange) -> NSRange { NSRange(location: r.location + range.location, length: r.length) }
        func rest(after r: NSRange) -> NSRange { NSRange(location: range.location + NSMaxRange(r), length: range.length - NSMaxRange(r)) }

        if let m = Self.heading.firstMatch(in: line, range: local) {
            let marker = m.range(at: 1)
            let markerText = (line as NSString).substring(with: marker)
            let level = markerText.filter { $0 == "#" }.count
            let font = Theme.font(size: (fontSize * Self.headingScale[level - 1]).rounded(), bold: true)
            let markerWidth = (markerText as NSString).size(withAttributes: [.font: font]).width
            storage.addAttributes([
                .font: font,
                .foregroundColor: Theme.heading,
                .paragraphStyle: paragraph(first: max(0, gutter - markerWidth), rest: gutter),
            ], range: range)
            faint(global(marker), storage)
            styleInline(rest(after: marker), text: text, in: storage)
        } else if Self.rule.firstMatch(in: line, range: local) != nil {
            faint(range, storage)
        } else if let m = Self.quote.firstMatch(in: line, range: local) {
            let marker = m.range(at: 1)
            color(Theme.muted, range, storage)
            faint(global(marker), storage)
            let body = rest(after: marker)
            if let c = Self.callout.firstMatch(in: text, options: .anchored, range: body) {
                storage.addAttribute(.foregroundColor, value: Theme.accent, range: c.range)
                addTraits(bold: true, to: c.range, in: storage)
            }
            styleInline(body, text: text, in: storage)
        } else if let m = Self.listItem.firstMatch(in: line, range: local) {
            let prefix = (line as NSString).substring(with: m.range)
            let columns = prefix.reduce(0) { $0 + ($1 == "\t" ? 4 : 1) }
            storage.addAttribute(.paragraphStyle, value: paragraph(first: gutter, rest: gutter + CGFloat(columns) * charWidth), range: range)
            faint(global(m.range(at: 2)), storage)
            let box = m.range(at: 4)
            if box.location != NSNotFound {
                faint(global(box), storage)
                if (line as NSString).substring(with: NSRange(location: box.location + 1, length: 1)) != " " {
                    let done = rest(after: m.range)
                    color(Theme.muted, done, storage)
                    storage.addAttribute(.strikethroughStyle, value: NSUnderlineStyle.single.rawValue, range: done)
                }
            }
            styleInline(rest(after: m.range), text: text, in: storage)
        } else if line.trimmingCharacters(in: .whitespaces).hasPrefix("|") {
            if Self.tableSeparator.firstMatch(in: line, range: local) != nil {
                faint(range, storage)
            } else {
                let ns = text as NSString
                let nextStart = NSMaxRange(ns.paragraphRange(for: range))
                let next = nextStart < ns.length ? ns.substring(with: ns.paragraphRange(for: NSRange(location: nextStart, length: 0))) : ""
                if Self.tableSeparator.firstMatch(in: next, range: NSRange(location: 0, length: (next as NSString).length)) != nil {
                    addTraits(bold: true, to: range, in: storage)
                    color(Theme.heading, range, storage)
                }
                for p in Self.pipe.matches(in: line, range: local) { faint(global(p.range), storage) }
                styleInline(range, text: text, in: storage)
                indentCellLineBreaks(in: line, at: range, storage)
            }
        } else if MarkdownTable.slashCommand.firstMatch(in: line, range: local) != nil {
            color(Theme.accent, range, storage)
        } else {
            if let m = Self.footnoteDefinition.firstMatch(in: line, range: local) { faint(global(m.range), storage) }
            styleInline(range, text: text, in: storage)
        }
    }

    private func styleInline(_ range: NSRange, text: String, in storage: NSTextStorage) {
        guard range.length > 0 else { return }
        var claimed: [NSRange] = []
        func free(_ r: NSRange) -> Bool { !claimed.contains { NSIntersectionRange($0, r).length > 0 } }
        func matches(_ regex: NSRegularExpression) -> [NSTextCheckingResult] {
            regex.matches(in: text, range: range).filter { free($0.range) }
        }

        for m in matches(Self.inlineCode) {
            color(Theme.code, m.range, storage)
            faint(m.range(at: 1), storage)
            faint(m.range(at: 3), storage)
            claimed.append(m.range)
        }

        for m in matches(Self.wikilink) {
            let target = m.range(at: 2), alias = m.range(at: 3)
            faint(m.range(at: 1), storage)
            faint(m.range(at: 4), storage)
            if alias.location != NSNotFound {
                faint(target, storage)
                faint(NSRange(location: alias.location, length: 1), storage)
                color(Theme.accent, NSRange(location: alias.location + 1, length: alias.length - 1), storage)
            } else {
                color(Theme.accent, target, storage)
            }
            storage.addAttribute(.clarityLink, value: (text as NSString).substring(with: target), range: m.range)
            claimed.append(m.range)
        }

        for m in matches(Self.link) {
            faint(m.range, storage)
            color(Theme.accent, m.range(at: 2), storage)
            storage.addAttribute(.clarityLink, value: (text as NSString).substring(with: m.range(at: 4)), range: m.range)
            claimed.append(m.range)
        }

        for m in matches(Self.bareURL) {
            color(Theme.accent, m.range, storage)
            let url = (text as NSString).substring(with: m.range).trimmingCharacters(in: CharacterSet(charactersIn: "<>"))
            storage.addAttribute(.clarityLink, value: url, range: m.range)
            claimed.append(m.range)
        }

        for m in matches(Self.footnoteRef) + matches(Self.lineBreak) { faint(m.range, storage) }

        for m in matches(Self.bold) {
            addTraits(bold: true, to: m.range, in: storage)
            faint(m.range(at: 1), storage)
            faint(m.range(at: 3), storage)
        }
        for m in matches(Self.italicStar) + matches(Self.italicUnderscore) {
            addTraits(italic: true, to: m.range, in: storage)
            faint(m.range(at: 1), storage)
            faint(m.range(at: 3), storage)
        }
        for m in matches(Self.strike) {
            color(Theme.muted, m.range(at: 2), storage)
            storage.addAttribute(.strikethroughStyle, value: NSUnderlineStyle.single.rawValue, range: m.range(at: 2))
            faint(m.range(at: 1), storage)
            faint(m.range(at: 3), storage)
        }
        for m in matches(Self.mark) {
            storage.addAttribute(.backgroundColor, value: Theme.highlight, range: m.range(at: 2))
            faint(m.range(at: 1), storage)
            faint(m.range(at: 3), storage)
        }
        for m in matches(Self.tag) { color(Theme.accent, m.range, storage) }
    }

    /// Indents a table row's continuation lines (after a line break in a cell) to where the broken
    /// cell's text starts. A paragraph has one indent, so that's the first broken cell in the row.
    private func indentCellLineBreaks(in line: String, at range: NSRange, _ storage: NSTextStorage) {
        guard let first = line.range(of: MarkdownTable.lineSeparator) else { return }
        let prefix = String(line[..<first.lowerBound])
        let cellStart = prefix.lastIndex(of: "|").map { prefix.index(after: $0) } ?? prefix.startIndex
        let column = prefix[..<cellStart].count + prefix[cellStart...].prefix { $0 == " " }.count
        storage.addAttribute(.paragraphStyle, value: paragraph(first: gutter, rest: gutter + CGFloat(column) * charWidth), range: range)
    }

    // MARK: Helpers

    private func color(_ color: NSColor, _ range: NSRange, _ storage: NSTextStorage) {
        storage.addAttribute(.foregroundColor, value: color, range: range)
    }

    private func faint(_ range: NSRange, _ storage: NSTextStorage) {
        color(Theme.faint, range, storage)
    }

    private func addTraits(bold: Bool = false, italic: Bool = false, to range: NSRange, in storage: NSTextStorage) {
        storage.enumerateAttribute(.font, in: range) { value, sub, _ in
            let current = value as? NSFont
            let traits = current?.fontDescriptor.symbolicTraits ?? []
            let font = Theme.font(
                size: current?.pointSize ?? fontSize, bold: bold || traits.contains(.bold), italic: italic || traits.contains(.italic)
            )
            storage.addAttribute(.font, value: font, range: sub)
        }
    }
}
