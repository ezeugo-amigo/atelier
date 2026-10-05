import Foundation

/// A remark left on a passage of a note, usually by an agent, that the writer reads and resolves.
/// Comments never touch the note itself: they live in a sidecar file and find their passage by
/// quoting it, so they survive edits anywhere else in the note.
struct Comment: Codable, Identifiable, Hashable {
    var id: String
    var author: String
    var body: String
    var anchor: Anchor
    var created: Date
    var resolved: Bool

    /// The quoted passage, with a little of the text around it to tell repeats apart
    /// (the W3C Web Annotation "text quote selector").
    struct Anchor: Codable, Hashable {
        var exact: String
        var prefix: String?
        var suffix: String?
    }

    init(id: String = UUID().uuidString, author: String, body: String, anchor: Anchor, created: Date = .now, resolved: Bool = false) {
        self.id = id
        self.author = author
        self.body = body
        self.anchor = anchor
        self.created = created
        self.resolved = resolved
    }

    /// Agents write these files by hand, so everything but the quote and the remark is optional.
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decodeIfPresent(String.self, forKey: .id) ?? UUID().uuidString
        author = try c.decodeIfPresent(String.self, forKey: .author) ?? "Agent"
        body = try c.decode(String.self, forKey: .body)
        anchor = try c.decode(Anchor.self, forKey: .anchor)
        created = try c.decodeIfPresent(Date.self, forKey: .created) ?? .now
        resolved = try c.decodeIfPresent(Bool.self, forKey: .resolved) ?? false
    }

    /// Where the quoted passage is in `text`: the occurrence whose surroundings best match the
    /// recorded prefix and suffix, or nil once the passage has been edited away.
    func range(in text: NSString) -> NSRange? {
        guard !anchor.exact.isEmpty else { return nil }
        var best: (range: NSRange, score: Int)?
        var search = NSRange(location: 0, length: text.length)
        while true {
            let found = text.range(of: anchor.exact, options: [], range: search)
            guard found.location != NSNotFound else { break }
            let score = Self.context(anchor.prefix, matches: text, before: found.location)
                + Self.context(anchor.suffix, matches: text, after: NSMaxRange(found))
            if score > (best?.score ?? -1) { best = (found, score) }
            let next = found.location + 1
            search = NSRange(location: next, length: text.length - next)
        }
        return best?.range
    }

    private static func context(_ prefix: String?, matches text: NSString, before location: Int) -> Int {
        guard let prefix = prefix as NSString?, prefix.length > 0 else { return 0 }
        let length = min(prefix.length, location)
        return text.substring(with: NSRange(location: location - length, length: length)) == prefix.substring(from: prefix.length - length) ? 1 : 0
    }

    private static func context(_ suffix: String?, matches text: NSString, after location: Int) -> Int {
        guard let suffix = suffix as NSString?, suffix.length > 0 else { return 0 }
        let length = min(suffix.length, text.length - location)
        return text.substring(with: NSRange(location: location, length: length)) == suffix.substring(to: length) ? 1 : 0
    }
}

/// Each note's comments, in `.clarity/comments/<note path>.json` under the workspace folder.
/// Hidden, so the note list skips it, but plain JSON an agent (or an MCP server) can write.
enum CommentFile {
    private struct Contents: Codable {
        var comments: [Comment]
    }

    static func url(for note: URL, in root: URL) -> URL {
        let relative = note.standardizedFileURL.path.dropFirst(root.standardizedFileURL.path.count).drop { $0 == "/" }
        return root.appending(path: ".clarity/comments/\(relative).json")
    }

    static func load(_ url: URL) -> [Comment] {
        guard let data = try? Data(contentsOf: url) else { return [] }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return (try? decoder.decode(Contents.self, from: data).comments) ?? []
    }

    static func save(_ comments: [Comment], to url: URL) throws {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try encoder.encode(Contents(comments: comments)).write(to: url, options: .atomic)
    }
}
