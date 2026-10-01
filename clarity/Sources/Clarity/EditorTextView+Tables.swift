import AppKit

/// Table editing: `/table` expands into a table, Tab / ⇧Tab move between cells (re-aligning the
/// table as they go), Return adds a row, and the Format ▸ Table menu adds or removes rows and columns.
extension EditorTextView {
    private struct TableContext {
        /// The table's lines, excluding the final newline.
        var range: NSRange
        var table: MarkdownTable
        var row: Int
        var column: Int
    }

    private func tableAtCaret() -> TableContext? {
        let ns = string as NSString
        let caret = selectedRange().location
        let current = ns.paragraphRange(for: NSRange(location: caret, length: 0))
        func isRow(_ line: NSRange) -> Bool {
            ns.substring(with: line).trimmingCharacters(in: .whitespaces).hasPrefix("|")
        }
        guard isRow(current) else { return nil }

        var first = current, last = current
        while first.location > 0 {
            let previous = ns.paragraphRange(for: NSRange(location: first.location - 1, length: 0))
            guard isRow(previous) else { break }
            first = previous
        }
        while NSMaxRange(last) < ns.length {
            let next = ns.paragraphRange(for: NSRange(location: NSMaxRange(last), length: 0))
            guard isRow(next) else { break }
            last = next
        }
        var range = NSRange(location: first.location, length: NSMaxRange(last) - first.location)
        if ns.substring(with: range).hasSuffix("\n") { range.length -= 1 }

        let lines = ns.substring(with: range).components(separatedBy: "\n").map { $0.trimmingCharacters(in: CharacterSet(charactersIn: "\r")) }
        guard let table = MarkdownTable(lines: lines) else { return nil }

        let lineIndex = ns.substring(with: NSRange(location: first.location, length: current.location - first.location)).filter { $0 == "\n" }.count
        let beforeCaret = ns.substring(with: NSRange(location: current.location, length: caret - current.location))
        var pipes = 0
        var escaped = false
        for c in beforeCaret {
            if c == "|" && !escaped { pipes += 1 }
            escaped = c == "\\" && !escaped
        }
        return TableContext(
            range: range, table: table, row: max(0, lineIndex - 1), column: min(max(pipes - 1, 0), table.columnCount - 1)
        )
    }

    /// Replaces `range` with the rendered table and selects the text of one cell, so typing
    /// overwrites it like a spreadsheet.
    /// Returns where the selected cell's text starts.
    @discardableResult
    private func write(_ table: MarkdownTable, over range: NSRange, selecting row: Int, _ column: Int) -> Int {
        let (lines, cellStarts) = table.render()
        let text = lines.joined(separator: "\n")
        if (string as NSString).substring(with: range) != text {
            insertText(text, replacementRange: range)
        }
        let line = MarkdownTable.line(ofRow: row)
        let cellStart = lines[..<line].reduce(range.location) { $0 + $1.utf16.count + 1 } + cellStarts[line][column]
        setSelectedRange(NSRange(location: cellStart, length: table.rows[row][column].utf16.count))
        return cellStart
    }

    // MARK: Hooks called from the editor's key handling

    /// Expands `/table` (or `/table 4x3`) when the caret ends a line containing only that.
    func expandSlashCommand() -> Bool {
        let ns = string as NSString
        let caret = selectedRange()
        let line = ns.paragraphRange(for: NSRange(location: caret.location, length: 0))
        let content = ns.substring(with: line).trimmingCharacters(in: .newlines)
        var contentRange = NSRange(location: line.location, length: (content as NSString).length)
        guard caret.length == 0, caret.location == NSMaxRange(contentRange),
              let match = MarkdownTable.slashCommand.firstMatch(in: content, range: NSRange(location: 0, length: contentRange.length))
        else { return false }

        func number(_ group: Int, default value: Int) -> Int {
            let r = match.range(at: group)
            return r.location == NSNotFound ? value : max(1, Int((content as NSString).substring(with: r)) ?? value)
        }
        undoManager?.beginUndoGrouping()
        let previousLine = line.location > 0 ? ns.substring(with: ns.paragraphRange(for: NSRange(location: line.location - 1, length: 0))) : ""
        if previousLine.trimmingCharacters(in: .whitespaces).hasPrefix("|") {
            insertText("\n", replacementRange: NSRange(location: line.location, length: 0))
            contentRange.location += 1
        }
        write(MarkdownTable(columns: number(1, default: 3), bodyRows: number(2, default: 2)), over: contentRange, selecting: 0, 0)
        undoManager?.endUndoGrouping()
        return true
    }

    func moveTableCell(by step: Int) -> Bool {
        guard var context = tableAtCaret() else { return false }
        var row = context.row, column = context.column + step
        if column == context.table.columnCount {
            column = 0
            row += 1
            if row == context.table.rows.count { context.table.insertRow(at: row) }
        } else if column < 0 {
            column = context.table.columnCount - 1
            row -= 1
            if row < 0 { (row, column) = (0, 0) }
        }
        write(context.table, over: context.range, selecting: row, column)
        return true
    }

    /// Return adds a row below; Return on an empty row removes it and leaves the table.
    func handleTableNewline() -> Bool {
        guard var context = tableAtCaret() else { return false }
        let row = context.row
        guard row > 0, context.table.rows[row].allSatisfy(\.isEmpty) else {
            context.table.insertRow(at: row + 1)
            write(context.table, over: context.range, selecting: row + 1, 0)
            return true
        }
        undoManager?.beginUndoGrouping()
        context.table.rows.remove(at: row)
        write(context.table, over: context.range, selecting: 0, 0)
        let end = NSMaxRange(tableAtCaret()?.range ?? context.range)
        // A blank line ends the table; GFM would read a line typed directly below as another row.
        insertText("\n\n", replacementRange: NSRange(location: end, length: 0))
        setSelectedRange(NSRange(location: end + 2, length: 0))
        undoManager?.endUndoGrouping()
        return true
    }

    /// A line break inside the cell (saved as `<br>`; see `MarkdownTable.lineSeparator`). The table
    /// is re-aligned right away so the columns stay lined up around the new line.
    func insertCellLineBreak() -> Bool {
        guard var context = tableAtCaret() else { return false }
        let ns = string as NSString
        let caret = selectedRange().location
        let line = ns.paragraphRange(for: NSRange(location: caret, length: 0))
        let beforeCaret = ns.substring(with: NSRange(location: line.location, length: caret - line.location))
        let inCell = beforeCaret.lastIndex(of: "|").map { beforeCaret[beforeCaret.index(after: $0)...] } ?? beforeCaret[...]
        let cell = context.table.rows[context.row][context.column] as NSString
        let offset = min(inCell.drop { $0 == " " }.utf16.count, cell.length)

        context.table.rows[context.row][context.column] = cell.substring(to: offset) + MarkdownTable.lineSeparator + cell.substring(from: offset)
        let cellStart = write(context.table, over: context.range, selecting: context.row, context.column)
        setSelectedRange(NSRange(location: cellStart + offset + 1, length: 0))
        return true
    }

    // MARK: Format ▸ Table

    @objc func insertTable(_ sender: Any?) {
        let ns = string as NSString
        let line = ns.paragraphRange(for: NSRange(location: selectedRange().location, length: 0))
        let content = ns.substring(with: line).trimmingCharacters(in: .newlines)
        var target = NSRange(location: line.location, length: (content as NSString).length)
        undoManager?.beginUndoGrouping()
        if !content.trimmingCharacters(in: .whitespaces).isEmpty {
            insertText("\n", replacementRange: NSRange(location: NSMaxRange(target), length: 0))
            target = NSRange(location: NSMaxRange(target) + 1, length: 0)
        }
        write(MarkdownTable(columns: 3, bodyRows: 2), over: target, selecting: 0, 0)
        undoManager?.endUndoGrouping()
    }

    @objc func addTableRow(_ sender: Any?) {
        guard var context = tableAtCaret() else { return NSSound.beep() }
        context.table.insertRow(at: context.row + 1)
        write(context.table, over: context.range, selecting: max(context.row + 1, 1), context.column)
    }

    @objc func addTableColumn(_ sender: Any?) {
        insertTableColumn(offset: 1)
    }

    @objc func addTableColumnLeft(_ sender: Any?) {
        insertTableColumn(offset: 0)
    }

    /// Inserts a column at the caret's column plus `offset` and selects its header cell.
    private func insertTableColumn(offset: Int) {
        guard var context = tableAtCaret() else { return NSSound.beep() }
        context.table.insertColumn(at: context.column + offset)
        write(context.table, over: context.range, selecting: 0, context.column + offset)
    }

    @objc func deleteTableRow(_ sender: Any?) {
        guard var context = tableAtCaret(), context.row > 0 else { return NSSound.beep() }
        context.table.rows.remove(at: context.row)
        write(context.table, over: context.range, selecting: min(context.row, context.table.rows.count - 1), context.column)
    }

    @objc func deleteTableColumn(_ sender: Any?) {
        guard var context = tableAtCaret(), context.table.columnCount > 1 else { return NSSound.beep() }
        context.table.removeColumn(at: context.column)
        write(context.table, over: context.range, selecting: context.row, min(context.column, context.table.columnCount - 1))
    }
}
