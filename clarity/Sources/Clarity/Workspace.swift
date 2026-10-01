import AppKit
import Observation

struct Note: Identifiable, Hashable {
    let url: URL
    let relativePath: String
    var id: URL { url }
    var name: String { url.deletingPathExtension().lastPathComponent }
    var folder: String { (relativePath as NSString).deletingLastPathComponent }
}

/// A folder of Markdown notes (an Obsidian vault works as-is) and the note open in the editor.
/// Edits autosave shortly after you stop typing; there is no "unsaved" state to manage.
@MainActor @Observable
final class Workspace {
    static let extensions: Set<String> = ["md", "markdown", "txt"]

    private(set) var root: URL
    /// Most recently modified first. Re-sorted on rescans, not on every save, so the list
    /// doesn't reshuffle under you while you type.
    private(set) var notes: [Note] = []
    private(set) var current: URL?
    private(set) var wordCount = 0
    /// Briefly replaces the word count, e.g. to confirm a copy.
    private(set) var statusMessage: String?
    @ObservationIgnored private var statusTask: Task<Void, Never>?
    var showQuickOpen = false

    func flash(_ message: String) {
        statusMessage = message
        statusTask?.cancel()
        statusTask = Task {
            try? await Task.sleep(for: .seconds(2))
            if !Task.isCancelled { statusMessage = nil }
        }
    }

    var showSidebar: Bool {
        didSet { UserDefaults.standard.set(showSidebar, forKey: "showSidebar") }
    }

    var focusMode = false {
        didSet { editor?.focusMode = focusMode }
    }

    var fontSize: CGFloat {
        didSet {
            UserDefaults.standard.set(fontSize, forKey: "fontSize")
            editor?.fontSize = fontSize
        }
    }

    static let defaultColumnWidth = 80
    static let columnWidths = 40...200

    /// Writing column width in characters. The window still caps it on small screens.
    var columnWidth: Int {
        didSet {
            UserDefaults.standard.set(columnWidth, forKey: "columnWidth")
            editor?.columnCharacters = columnWidth
        }
    }

    var darkMode: Bool {
        didSet {
            UserDefaults.standard.set(darkMode, forKey: "darkMode")
            applyAppearance()
        }
    }

    @ObservationIgnored weak var editor: EditorTextView? {
        didSet { showInEditor() }
    }

    /// The note as it is (or will be) on disk; the editor shows `MarkdownTable.editorText(fromDisk:)`.
    @ObservationIgnored private var text = ""
    @ObservationIgnored private var savedText = ""
    /// The editor's latest contents, not yet folded into `text`.
    @ObservationIgnored private var unsyncedEditorText: String?
    @ObservationIgnored private var saveTask: Task<Void, Never>?
    @ObservationIgnored private var back: [URL] = []
    @ObservationIgnored private var forward: [URL] = []

    init() {
        let defaults = UserDefaults.standard
        root = defaults.url(forKey: "root")
            ?? FileManager.default.homeDirectoryForCurrentUser.appending(path: "Documents/Clarity", directoryHint: .isDirectory)
        showSidebar = defaults.object(forKey: "showSidebar") as? Bool ?? true
        fontSize = defaults.object(forKey: "fontSize") as? CGFloat ?? 16
        columnWidth = defaults.object(forKey: "columnWidth") as? Int ?? Self.defaultColumnWidth
        darkMode = defaults.object(forKey: "darkMode") as? Bool ?? true

        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        scan()
        let last = defaults.url(forKey: "lastNote")
        show(notes.first { $0.url == last }?.url ?? notes.first?.url)
    }

    func applyAppearance() {
        NSApp.appearance = NSAppearance(named: darkMode ? .darkAqua : .aqua)
    }

    // MARK: Opening

    func open(_ url: URL?) {
        guard let url, url != current else { return }
        if let current {
            back.append(current)
            forward.removeAll()
        }
        show(url)
    }

    /// Loads `url` into the editor without touching history.
    private func show(_ url: URL?) {
        save()
        current = url
        text = url.flatMap { try? String(contentsOf: $0, encoding: .utf8) } ?? ""
        savedText = text
        wordCount = Self.countWords(MarkdownTable.editorText(fromDisk: text))
        UserDefaults.standard.set(url, forKey: "lastNote")
        showInEditor()
    }

    private func showInEditor() {
        editor?.load(MarkdownTable.editorText(fromDisk: text))
        updateTitle()
    }

    private func updateTitle() {
        editor?.window?.title = current?.deletingPathExtension().lastPathComponent ?? "Clarity"
    }

    /// Opens a file or folder handed to the app from Finder, `open -a`, or the command line.
    /// A file outside the current folder makes its own folder the workspace.
    func openExternal(_ url: URL) {
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory) else { return }
        if isDirectory.boolValue {
            setRoot(url)
            return
        }
        if !url.resolvingSymlinksInPath().path.hasPrefix(root.resolvingSymlinksInPath().path + "/") {
            setRoot(url.deletingLastPathComponent())
        }
        open(url)
    }

    func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.prompt = "Open Folder"
        panel.directoryURL = root
        if panel.runModal() == .OK, let url = panel.url { setRoot(url) }
    }

    private func setRoot(_ url: URL) {
        save()
        root = url
        UserDefaults.standard.set(url, forKey: "root")
        back.removeAll()
        forward.removeAll()
        scan()
        show(notes.first?.url)
    }

    // MARK: Navigation

    func goBack() {
        while let url = back.popLast() {
            guard FileManager.default.fileExists(atPath: url.path) else { continue }
            if let current { forward.append(current) }
            show(url)
            return
        }
    }

    func goForward() {
        while let url = forward.popLast() {
            guard FileManager.default.fileExists(atPath: url.path) else { continue }
            if let current { back.append(current) }
            show(url)
            return
        }
    }

    func openAdjacent(_ offset: Int) {
        guard !notes.isEmpty else { return }
        let index = notes.firstIndex { $0.url == current }.map { $0 + offset } ?? 0
        open(notes[min(max(index, 0), notes.count - 1)].url)
    }

    /// Follows a wikilink target or Markdown link: web URLs open in the browser, everything else
    /// resolves to a note (relative path first, then by name anywhere in the folder), creating it
    /// if it doesn't exist yet, as Obsidian does.
    func follow(_ target: String) {
        if target.contains("://") || target.hasPrefix("mailto:") {
            if let url = URL(string: target) { NSWorkspace.shared.open(url) }
            return
        }
        var path = target.removingPercentEncoding ?? target
        if let hash = path.firstIndex(of: "#") { path = String(path[..<hash]) }
        path = path.trimmingCharacters(in: .whitespaces)
        guard !path.isEmpty else { return }

        if let dir = current?.deletingLastPathComponent() {
            let candidate = dir.appending(path: path).standardized
            if Self.extensions.contains(candidate.pathExtension.lowercased()), FileManager.default.fileExists(atPath: candidate.path) {
                open(candidate)
                return
            }
        }
        let name = Self.extensions.contains((path as NSString).pathExtension.lowercased()) ? (path as NSString).deletingPathExtension : path
        open(resolve(name) ?? createNote(named: name))
    }

    private func resolve(_ name: String) -> URL? {
        let key = name.lowercased()
        return notes.first {
            $0.name.lowercased() == key || ($0.relativePath as NSString).deletingPathExtension.lowercased() == key
        }?.url
    }

    // MARK: Editing

    /// Called on every keystroke, so it only records the text: converting it for disk and counting
    /// words are whole-document passes, done once typing pauses (`syncText`).
    func textDidChange(_ newText: String) {
        unsyncedEditorText = newText
        if current == nil {
            syncText()
            current = createNote(contents: text)
            savedText = text
            updateTitle()
        }
        saveTask?.cancel()
        saveTask = Task {
            try? await Task.sleep(for: .milliseconds(500))
            if !Task.isCancelled { save() }
        }
    }

    private func syncText() {
        guard let editorText = unsyncedEditorText else { return }
        unsyncedEditorText = nil
        text = MarkdownTable.diskText(fromEditor: editorText)
        wordCount = Self.countWords(editorText)
    }

    func save() {
        saveTask?.cancel()
        saveTask = nil
        syncText()
        guard let current, text != savedText else { return }
        do {
            try text.write(to: current, atomically: true, encoding: .utf8)
            savedText = text
        } catch {
            NSAlert(error: error).runModal()
        }
    }

    /// Picks up changes made outside the app (sync, git, another editor) when we regain focus.
    func refresh() {
        scan()
        syncText()
        guard let current else { return }
        guard FileManager.default.fileExists(atPath: current.path) else {
            text = ""
            savedText = ""
            show(notes.first?.url)
            return
        }
        guard text == savedText, let disk = try? String(contentsOf: current, encoding: .utf8), disk != savedText else { return }
        text = disk
        savedText = disk
        let shown = MarkdownTable.editorText(fromDisk: disk)
        wordCount = Self.countWords(shown)
        editor?.load(shown, keepSelection: true)
    }

    // MARK: Files

    func newNote() {
        open(createNote())
        focusEditor()
    }

    @discardableResult
    private func createNote(named name: String = "Untitled", contents: String = "") -> URL {
        var url = root.appending(path: "\(name).md")
        var n = 2
        while FileManager.default.fileExists(atPath: url.path) {
            url = root.appending(path: "\(name) \(n).md")
            n += 1
        }
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        FileManager.default.createFile(atPath: url.path, contents: Data(contents.utf8))
        scan()
        return url
    }

    func rename(_ target: URL? = nil) {
        guard let url = target ?? current else { return }
        save()
        let alert = NSAlert()
        alert.messageText = "Rename note"
        alert.addButton(withTitle: "Rename")
        alert.addButton(withTitle: "Cancel")
        let field = NSTextField(string: url.deletingPathExtension().lastPathComponent)
        field.frame = NSRect(x: 0, y: 0, width: 280, height: 24)
        alert.accessoryView = field
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return }

        let name = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        let destination = url.deletingLastPathComponent().appending(path: name).appendingPathExtension(url.pathExtension)
        guard destination != url else { return }
        do {
            try FileManager.default.moveItem(at: url, to: destination)
        } catch {
            NSAlert(error: error).runModal()
            return
        }
        back = back.map { $0 == url ? destination : $0 }
        forward = forward.map { $0 == url ? destination : $0 }
        if current == url {
            current = destination
            UserDefaults.standard.set(destination, forKey: "lastNote")
            updateTitle()
        }
        scan()
    }

    func trash(_ target: URL? = nil) {
        guard let url = target ?? current else { return }
        let next = notes.firstIndex { $0.url == url }.flatMap { i in notes.indices.contains(i + 1) ? notes[i + 1].url : nil }
        if url == current {
            saveTask?.cancel()
            syncText()
            savedText = text
        }
        do {
            try FileManager.default.trashItem(at: url, resultingItemURL: nil)
        } catch {
            NSAlert(error: error).runModal()
            return
        }
        scan()
        if url == current { show(next ?? notes.first?.url) }
    }

    func reveal(_ target: URL? = nil) {
        guard let url = target ?? current else { return }
        NSWorkspace.shared.activateFileViewerSelecting([url])
    }

    /// Enumerates by relative path and builds URLs from `root`, so note URLs compare equal to the ones
    /// we open even when the folder sits behind a symlink (e.g. /tmp → /private/tmp).
    private func scan() {
        guard let enumerator = FileManager.default.enumerator(atPath: root.path) else {
            notes = []
            return
        }
        var found: [(note: Note, modified: Date)] = []
        while let relative = enumerator.nextObject() as? String {
            let attributes = enumerator.fileAttributes
            let type = attributes?[.type] as? FileAttributeType
            if (relative as NSString).lastPathComponent.hasPrefix(".") {
                if type == .typeDirectory { enumerator.skipDescendants() }
                continue
            }
            guard type == .typeRegular, Self.extensions.contains((relative as NSString).pathExtension.lowercased()) else { continue }
            let modified = attributes?[.modificationDate] as? Date ?? .distantPast
            found.append((Note(url: root.appending(path: relative), relativePath: relative), modified))
        }
        notes = found.sorted { $0.modified > $1.modified }.map(\.note)
    }

    // MARK: UI

    func toggleQuickOpen() {
        showQuickOpen.toggle()
        if !showQuickOpen { focusEditor() }
    }

    func focusEditor() {
        DispatchQueue.main.async { [weak self] in
            guard let editor = self?.editor else { return }
            editor.window?.makeFirstResponder(editor)
        }
    }

    /// One pass over UTF-16 without allocating, since notes can be large.
    private static func countWords(_ text: String) -> Int {
        var count = 0
        var inWord = false
        for unit in text.utf16 {
            switch unit {
            case 9, 10, 13, 32, 0xA0, 0x2028, 0x2029:
                inWord = false
            default:
                if !inWord { count += 1 }
                inWord = true
            }
        }
        return count
    }
}
