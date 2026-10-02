import AppKit
import CryptoKit

/// A change to a note. Offsets are UTF-16 units into the editor's text, where a line break inside
/// a table cell is U+2028 rather than the `<br>` saved on disk.
enum Op: Equatable {
    /// `del` (the text that was at `at`) replaced by `ins`. Either may be empty.
    case edit(at: Int, del: String, ins: String)
    /// The caret or selection moved somewhere other than the end of the last edit.
    case select(NSRange)
}

struct Entry: Equatable {
    /// Milliseconds since 1970.
    var time: Int64
    var op: Op
}

extension Entry: Codable {
    private enum CodingKeys: String, CodingKey { case t, at, del, ins, sel }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        time = try c.decode(Int64.self, forKey: .t)
        if let sel = try c.decodeIfPresent([Int].self, forKey: .sel), sel.count == 2 {
            op = .select(NSRange(location: sel[0], length: sel[1]))
        } else {
            op = .edit(
                at: try c.decode(Int.self, forKey: .at),
                del: try c.decodeIfPresent(String.self, forKey: .del) ?? "",
                ins: try c.decodeIfPresent(String.self, forKey: .ins) ?? ""
            )
        }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(time, forKey: .t)
        switch op {
        case let .edit(at, del, ins):
            try c.encode(at, forKey: .at)
            if !del.isEmpty { try c.encode(del, forKey: .del) }
            if !ins.isEmpty { try c.encode(ins, forKey: .ins) }
        case let .select(range):
            try c.encode([range.location, range.length], forKey: .sel)
        }
    }
}

/// Every edit and caret move made to one note, so its writing can be replayed from an empty page.
/// Kept as JSON lines (one `Entry` per line, appended as you write) in
/// ~/Library/Application Support/Clarity/History, keyed by the note's path.
///
/// The log also holds the document it describes. Whenever that drifts from the editor (the file
/// changed outside Clarity, or the app quit before a flush), one edit covering the difference is
/// appended, so replaying always ends at the text you see.
@MainActor
final class OpLog {
    var note: URL
    private(set) var entries: [Entry]
    /// The document as of the last entry.
    private var text: NSMutableString
    /// The selection as of the last entry: an edit leaves the caret after its inserted text.
    private var selection = NSRange(location: 0, length: 0)
    private var flushed: Int
    /// Notes you only read never get a log file.
    private var edited = false

    init(note: URL, text current: String) {
        self.note = note
        entries = Self.read(Self.file(for: note))
        flushed = entries.count
        text = NSMutableString()
        for entry in entries { apply(entry.op) }
        reconcile(with: current)
    }

    // MARK: Recording

    /// Records an edit already made to `document`: `range` is the new text and `delta` the change in
    /// length, as the text storage reports them. The replaced text comes from the log's own copy.
    /// Takes the editor's storage as-is, since bridging it to a String copies the whole note.
    func record(edited range: NSRange, changeInLength delta: Int, in document: NSString) {
        edited = true
        let old = NSRange(location: range.location, length: range.length - delta)
        guard old.length >= 0, NSMaxRange(old) <= text.length, text.length + delta == document.length else {
            reconcile(with: document as String)
            return
        }
        append(.edit(at: old.location, del: text.substring(with: old), ins: document.substring(with: range)))
    }

    func record(selection range: NSRange) {
        guard range != selection, NSMaxRange(range) <= text.length else { return }
        append(.select(range))
    }

    /// Appends one edit turning the log's document into `current`, if they differ.
    func reconcile(with current: String) {
        let new = current as NSString
        guard !text.isEqual(to: current) else { return }
        let oldLength = text.length, newLength = new.length
        var prefix = 0
        while prefix < min(oldLength, newLength), text.character(at: prefix) == new.character(at: prefix) { prefix += 1 }
        var suffix = 0
        while suffix < min(oldLength, newLength) - prefix,
              text.character(at: oldLength - 1 - suffix) == new.character(at: newLength - 1 - suffix) { suffix += 1 }
        // Don't split a surrogate pair: half an emoji isn't a valid String.
        if prefix > 0, UTF16.isLeadSurrogate(new.character(at: prefix - 1)) { prefix -= 1 }
        if suffix > 0, UTF16.isTrailSurrogate(new.character(at: newLength - suffix)) { suffix -= 1 }
        append(.edit(
            at: prefix,
            del: text.substring(with: NSRange(location: prefix, length: oldLength - prefix - suffix)),
            ins: new.substring(with: NSRange(location: prefix, length: newLength - prefix - suffix))
        ))
    }

    private func append(_ op: Op) {
        entries.append(Entry(time: Int64((Date().timeIntervalSince1970 * 1000).rounded()), op: op))
        apply(op)
    }

    private func apply(_ op: Op) {
        switch op {
        case let .edit(at, del, ins):
            let range = NSRange(location: at, length: (del as NSString).length)
            guard NSMaxRange(range) <= text.length else { return }
            text.replaceCharacters(in: range, with: ins)
            selection = NSRange(location: at + (ins as NSString).length, length: 0)
        case let .select(range):
            selection = range
        }
    }

    // MARK: Storage

    func flush() {
        let file = Self.file(for: note)
        guard flushed < entries.count, edited || FileManager.default.fileExists(atPath: file.path) else { return }
        let encoder = JSONEncoder()
        var data = Data()
        for entry in entries[flushed...] {
            guard let line = try? encoder.encode(entry) else { continue }
            data.append(line)
            data.append(0x0A)
        }
        do {
            try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
            if !FileManager.default.fileExists(atPath: file.path) {
                FileManager.default.createFile(atPath: file.path, contents: nil)
            }
            let handle = try FileHandle(forWritingTo: file)
            defer { try? handle.close() }
            try handle.seekToEnd()
            try handle.write(contentsOf: data)
            flushed = entries.count
        } catch {
            NSLog("Clarity: couldn't write history for %@: %@", note.path, error.localizedDescription)
        }
    }

    static let directory = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        .appending(path: "Clarity/History", directoryHint: .isDirectory)

    static func file(for note: URL) -> URL {
        let digest = SHA256.hash(data: Data(note.standardizedFileURL.path.utf8))
        return directory.appending(path: digest.prefix(16).map { String(format: "%02x", $0) }.joined() + ".jsonl")
    }

    static func move(from note: URL, to destination: URL) {
        let source = file(for: note), target = file(for: destination)
        guard FileManager.default.fileExists(atPath: source.path) else { return }
        try? FileManager.default.removeItem(at: target)
        try? FileManager.default.moveItem(at: source, to: target)
    }

    static func remove(for note: URL) {
        try? FileManager.default.removeItem(at: file(for: note))
    }

    /// Skips lines that don't parse, such as one cut short by a crash; `reconcile` covers the gap.
    private static func read(_ file: URL) -> [Entry] {
        guard let data = try? Data(contentsOf: file) else { return [] }
        let decoder = JSONDecoder()
        return data.split(separator: 0x0A).compactMap { try? decoder.decode(Entry.self, from: $0) }
    }

    // MARK: Export

    /// The log in ezeugo.dev's replay format: a JSON array of cursor-relative ops, where `Insert`
    /// and `Delete` act on the range set by the preceding `CursorUpdate`, and each op carries its inverse.
    func exportForSite() throws -> Data {
        struct Span: Encodable { let start: Int, end: Int }
        struct Undo: Encodable { let type: String; var range: Span?; var value: String? }
        struct SiteOp: Encodable {
            let type: String
            var range: Span?
            var value: String?
            let undo: Undo
            let version = 1
            let position: Int
            let timestamp: Int64
        }

        var ops: [SiteOp] = []
        var cursor = Span(start: 0, end: 0)
        func moveCursor(to range: Span, at time: Int64) {
            ops.append(SiteOp(type: "CursorUpdate", range: range, undo: Undo(type: "CursorUpdate", range: cursor), position: ops.count + 1, timestamp: time))
            cursor = range
        }
        for entry in entries {
            switch entry.op {
            case let .select(range):
                moveCursor(to: Span(start: range.location, end: NSMaxRange(range)), at: entry.time)
            case let .edit(at, del, ins):
                let target = Span(start: at, end: at + (del as NSString).length)
                if target.start != cursor.start || target.end != cursor.end { moveCursor(to: target, at: entry.time) }
                if !del.isEmpty {
                    ops.append(SiteOp(type: "Delete", undo: Undo(type: "Insert", value: del), position: ops.count + 1, timestamp: entry.time))
                }
                if !ins.isEmpty {
                    ops.append(SiteOp(type: "Insert", value: ins, undo: Undo(type: "Delete"), position: ops.count + 1, timestamp: entry.time))
                }
                let caret = at + (ins as NSString).length
                moveCursor(to: Span(start: caret, end: caret), at: entry.time)
            }
        }
        return try JSONEncoder().encode(ops)
    }
}
