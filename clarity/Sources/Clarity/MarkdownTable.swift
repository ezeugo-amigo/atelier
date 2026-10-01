import Foundation

/// A GitHub-flavored Markdown table: a header row, a separator row carrying column alignment, and
/// body rows. Rendering pads every column to a common width so the pipes line up in a mono font.
struct MarkdownTable {
    enum Alignment { case none, left, center, right }

    /// The header row first, then body rows. Every row has `columnCount` cells.
    var rows: [[String]]
    var alignments: [Alignment]

    var columnCount: Int { alignments.count }

    /// `/table` or `/table 4x3` (columns × body rows) alone on a line.
    static let slashCommand = try! NSRegularExpression(pattern: #"^[ \t]*/table(?:[ \t]+(\d{1,2})[ \t]*[x×][ \t]*(\d{1,3}))?[ \t]*$"#)

    init(columns: Int, bodyRows: Int) {
        rows = [(1...columns).map { "Column \($0)" }] + Array(repeating: Array(repeating: "", count: columns), count: bodyRows)
        alignments = Array(repeating: .none, count: columns)
    }

    init?(lines: [String]) {
        guard lines.count >= 2, let alignments = Self.alignments(lines[1]) else { return nil }
        let rows = ([lines[0]] + lines.dropFirst(2)).map { Self.cells(Self.withLineSeparators($0)) }
        let count = max(alignments.count, rows.map(\.count).max() ?? 0)
        self.rows = rows.map { $0 + Array(repeating: "", count: count - $0.count) }
        self.alignments = alignments + Array(repeating: .none, count: count - alignments.count)
    }

    /// Splits a row on unescaped pipes, dropping the optional outer ones.
    static func cells(_ line: String) -> [String] {
        var row = Substring(line.trimmingCharacters(in: .whitespaces))
        if row.hasPrefix("|") { row = row.dropFirst() }
        if row.hasSuffix("|") && !row.hasSuffix("\\|") { row = row.dropLast() }
        var cells: [String] = []
        var cell = ""
        var escaped = false
        for c in row {
            if c == "|" && !escaped {
                cells.append(cell)
                cell = ""
            } else {
                cell.append(c)
            }
            escaped = c == "\\" && !escaped
        }
        cells.append(cell)
        return cells.map { $0.trimmingCharacters(in: .whitespaces) }
    }

    private static func alignments(_ line: String) -> [Alignment]? {
        var result: [Alignment] = []
        for cell in cells(line) {
            guard cell.range(of: #"^:?-+:?$"#, options: .regularExpression) != nil else { return nil }
            switch (cell.hasPrefix(":"), cell.hasSuffix(":")) {
            case (true, true): result.append(.center)
            case (false, true): result.append(.right)
            case (true, false): result.append(.left)
            case (false, false): result.append(.none)
            }
        }
        return result
    }

    /// The line a row renders on; the separator sits between the header and the body.
    static func line(ofRow row: Int) -> Int { row == 0 ? 0 : row + 1 }

    // MARK: Line breaks in cells
    //
    // A table row has to stay on one line in the file, so a break inside a cell is saved as `<br>`.
    // In the editor it's U+2028 LINE SEPARATOR instead: a real line break that stays inside the
    // row's paragraph, so the text system wraps it under the cell and treats it as one character.

    static let lineSeparator = "\u{2028}"

    static func editorText(fromDisk text: String) -> String {
        text.components(separatedBy: "\n").map { line in
            guard line.trimmingCharacters(in: .whitespaces).hasPrefix("|") else { return line }
            return withLineSeparators(line)
        }.joined(separator: "\n")
    }

    static func diskText(fromEditor text: String) -> String {
        text.replacingOccurrences(of: lineSeparator, with: "<br>")
    }

    private static func withLineSeparators(_ text: String) -> String {
        text.replacingOccurrences(of: #"<br\s*/?>"#, with: lineSeparator, options: [.regularExpression, .caseInsensitive])
    }

    static func visualLines(of cell: String) -> [String] {
        cell.components(separatedBy: lineSeparator)
    }

    /// Rendered lines, plus the UTF-16 offset at which each cell's text starts on its line.
    /// Columns are as wide as their longest visual line, and padding follows a cell's last
    /// visual line, so pipes stay aligned when a cell breaks across lines.
    func render() -> (lines: [String], cellStarts: [[Int]]) {
        let widths = (0..<columnCount).map { c in
            max(3, rows.flatMap { Self.visualLines(of: $0[c]) }.map(\.count).max() ?? 0)
        }
        func line(_ cells: [String]) -> (String, [Int]) {
            var text = "|"
            var starts: [Int] = []
            for (c, cell) in cells.enumerated() {
                text += " "
                starts.append(text.utf16.count)
                let lastLine = Self.visualLines(of: cell).last ?? ""
                text += cell + String(repeating: " ", count: widths[c] - lastLine.count) + " |"
            }
            return (text, starts)
        }
        let separator = zip(alignments, widths).map { alignment, width in
            switch alignment {
            case .none: String(repeating: "-", count: width)
            case .left: ":" + String(repeating: "-", count: width - 1)
            case .right: String(repeating: "-", count: width - 1) + ":"
            case .center: ":" + String(repeating: "-", count: width - 2) + ":"
            }
        }
        let rendered = [line(rows[0]), line(separator)] + rows.dropFirst().map(line)
        return (rendered.map(\.0), rendered.map(\.1))
    }

    mutating func insertRow(at index: Int) {
        rows.insert(Array(repeating: "", count: columnCount), at: max(1, index))
    }

    mutating func insertColumn(at index: Int) {
        for r in rows.indices { rows[r].insert("", at: index) }
        alignments.insert(.none, at: index)
    }

    mutating func removeColumn(at index: Int) {
        for r in rows.indices { rows[r].remove(at: index) }
        alignments.remove(at: index)
    }
}
