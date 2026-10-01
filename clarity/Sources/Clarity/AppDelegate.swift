import AppKit
import SwiftUI

@main
enum Clarity {
    @MainActor static func main() {
        let app = NSApplication.shared
        let delegate = AppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.regular)
        app.run()
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private lazy var workspace = Workspace()
    private var window: NSWindow?

    func applicationWillFinishLaunching(_ notification: Notification) {
        Theme.registerFonts()
        NSApp.mainMenu = makeMainMenu()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        workspace.applyAppearance()
        if let path = CommandLine.arguments.dropFirst().first(where: { !$0.hasPrefix("-") }) {
            let cwd = URL(filePath: FileManager.default.currentDirectoryPath, directoryHint: .isDirectory)
            workspace.openExternal(URL(filePath: (path as NSString).expandingTildeInPath, relativeTo: cwd).standardizedFileURL)
        }

        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1100, height: 760),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.backgroundColor = Theme.background
        window.isReleasedWhenClosed = false
        window.minSize = NSSize(width: 480, height: 320)
        window.collectionBehavior = .fullScreenPrimary
        let hosting = NSHostingView(rootView: ContentView(workspace: workspace))
        hosting.sizingOptions = []
        window.contentView = hosting
        window.center()
        window.setFrameAutosaveName("ClarityMainWindow")
        window.makeKeyAndOrderFront(nil)
        self.window = window

        NSApp.activate()
        workspace.focusEditor()
    }

    func application(_ application: NSApplication, open urls: [URL]) {
        if let url = urls.first { workspace.openExternal(url) }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
    func applicationWillTerminate(_ notification: Notification) { workspace.save() }
    func applicationDidResignActive(_ notification: Notification) { workspace.save() }
    func applicationDidBecomeActive(_ notification: Notification) { workspace.refresh() }

    // MARK: Actions

    @objc private func newNote(_ sender: Any?) { workspace.newNote() }
    @objc private func quickOpen(_ sender: Any?) { workspace.toggleQuickOpen() }
    @objc private func openFolder(_ sender: Any?) { workspace.chooseFolder() }
    @objc private func renameNote(_ sender: Any?) { workspace.rename() }
    @objc private func revealNote(_ sender: Any?) { workspace.reveal() }
    @objc private func trashNote(_ sender: Any?) { workspace.trash() }
    @objc private func saveNote(_ sender: Any?) { workspace.save() }
    @objc private func toggleSidebar(_ sender: Any?) { workspace.showSidebar.toggle() }
    @objc private func toggleFocusMode(_ sender: Any?) { workspace.focusMode.toggle() }
    @objc private func toggleDarkMode(_ sender: Any?) { workspace.darkMode.toggle() }
    @objc private func biggerText(_ sender: Any?) { workspace.fontSize = min(workspace.fontSize + 1, 28) }
    @objc private func smallerText(_ sender: Any?) { workspace.fontSize = max(workspace.fontSize - 1, 11) }
    @objc private func actualSizeText(_ sender: Any?) { workspace.fontSize = 16 }
    @objc private func widerColumn(_ sender: Any?) { adjustColumn(by: 10) }
    @objc private func narrowerColumn(_ sender: Any?) { adjustColumn(by: -10) }
    @objc private func defaultColumn(_ sender: Any?) { workspace.columnWidth = Workspace.defaultColumnWidth }

    private func adjustColumn(by delta: Int) {
        let range = Workspace.columnWidths
        workspace.columnWidth = min(max(workspace.columnWidth + delta, range.lowerBound), range.upperBound)
    }
    @objc private func goBack(_ sender: Any?) { workspace.goBack() }
    @objc private func goForward(_ sender: Any?) { workspace.goForward() }
    @objc private func previousNote(_ sender: Any?) { workspace.openAdjacent(-1) }
    @objc private func nextNote(_ sender: Any?) { workspace.openAdjacent(1) }
    @objc private func focusEditor(_ sender: Any?) { workspace.focusEditor() }

    // MARK: Menu

    private func makeMainMenu() -> NSMenu {
        let up = String(UnicodeScalar(NSUpArrowFunctionKey)!)
        let down = String(UnicodeScalar(NSDownArrowFunctionKey)!)
        let left = String(UnicodeScalar(NSLeftArrowFunctionKey)!)
        let right = String(UnicodeScalar(NSRightArrowFunctionKey)!)
        let backspace = "\u{8}"

        let main = NSMenu()
        add("Clarity", to: main, [
            item("About Clarity", #selector(NSApplication.orderFrontStandardAboutPanel(_:)), "", target: nil),
            .separator(),
            item("Hide Clarity", #selector(NSApplication.hide(_:)), "h", target: nil),
            item("Hide Others", #selector(NSApplication.hideOtherApplications(_:)), "h", [.command, .option], target: nil),
            item("Show All", #selector(NSApplication.unhideAllApplications(_:)), "", target: nil),
            .separator(),
            item("Quit Clarity", #selector(NSApplication.terminate(_:)), "q", target: nil),
        ])
        add("File", to: main, [
            item("New Note", #selector(newNote(_:)), "n"),
            item("Open Note…", #selector(quickOpen(_:)), "o"),
            hidden(item("Open Note…", #selector(quickOpen(_:)), "p")),
            item("Open Folder…", #selector(openFolder(_:)), "o", [.command, .shift]),
            .separator(),
            item("Rename…", #selector(renameNote(_:)), "r", [.command, .shift]),
            item("Reveal in Finder", #selector(revealNote(_:)), "r", [.command, .option]),
            item("Move to Trash", #selector(trashNote(_:)), backspace, [.command, .shift]),
            .separator(),
            item("Save", #selector(saveNote(_:)), "s"),
            item("Close Window", #selector(NSWindow.performClose(_:)), "w", target: nil),
        ])
        add("Edit", to: main, [
            item("Undo", Selector(("undo:")), "z", target: nil),
            item("Redo", Selector(("redo:")), "z", [.command, .shift], target: nil),
            .separator(),
            item("Cut", #selector(NSText.cut(_:)), "x", target: nil),
            item("Copy", #selector(NSText.copy(_:)), "c", target: nil),
            item("Copy as Rich Text", #selector(EditorTextView.copyAsRichText(_:)), "c", [.command, .shift], target: nil),
            item("Paste", #selector(NSText.paste(_:)), "v", target: nil),
            item("Select All", #selector(NSText.selectAll(_:)), "a", target: nil),
            .separator(),
            find("Find…", .showFindInterface, "f"),
            find("Find and Replace…", .showReplaceInterface, "f", [.command, .option]),
            find("Find Next", .nextMatch, "g"),
            find("Find Previous", .previousMatch, "g", [.command, .shift]),
            find("Use Selection for Find", .setSearchString, "e"),
        ])
        add("Format", to: main, [
            item("Bold", #selector(EditorTextView.formatBold(_:)), "b", target: nil),
            item("Italic", #selector(EditorTextView.formatItalic(_:)), "i", target: nil),
            item("Strikethrough", #selector(EditorTextView.formatStrikethrough(_:)), "x", [.command, .shift], target: nil),
            item("Highlight", #selector(EditorTextView.formatHighlight(_:)), "h", [.command, .shift], target: nil),
            item("Inline Code", #selector(EditorTextView.formatCode(_:)), "c", [.command, .option], target: nil),
            item("Link", #selector(EditorTextView.formatLink(_:)), "k", target: nil),
            .separator(),
            item("Toggle Task", #selector(EditorTextView.toggleTask(_:)), "l", target: nil),
            .separator(),
            submenu("Table", [
                item("Insert Table", #selector(EditorTextView.insertTable(_:)), "t", [.command, .option], target: nil),
                .separator(),
                item("Add Row", #selector(EditorTextView.addTableRow(_:)), "\r", [.command, .option], target: nil),
                item("Add Column Left", #selector(EditorTextView.addTableColumnLeft(_:)), left, [.command, .option], target: nil),
                item("Add Column Right", #selector(EditorTextView.addTableColumn(_:)), right, [.command, .option], target: nil),
                item("Delete Row", #selector(EditorTextView.deleteTableRow(_:)), backspace, [.command, .option], target: nil),
                item("Delete Column", #selector(EditorTextView.deleteTableColumn(_:)), backspace, [.command, .option, .shift], target: nil),
            ]),
        ])
        add("View", to: main, [
            item("Toggle Sidebar", #selector(toggleSidebar(_:)), "\\"),
            item("Focus Mode", #selector(toggleFocusMode(_:)), "d"),
            item("Toggle Dark Mode", #selector(toggleDarkMode(_:)), "d", [.command, .shift]),
            .separator(),
            item("Bigger", #selector(biggerText(_:)), "="),
            hidden(item("Bigger", #selector(biggerText(_:)), "+")),
            item("Smaller", #selector(smallerText(_:)), "-"),
            item("Actual Size", #selector(actualSizeText(_:)), "0"),
            .separator(),
            item("Wider Column", #selector(widerColumn(_:)), "=", [.command, .option]),
            hidden(item("Wider Column", #selector(widerColumn(_:)), "+", [.command, .option])),
            item("Narrower Column", #selector(narrowerColumn(_:)), "-", [.command, .option]),
            item("Default Column Width", #selector(defaultColumn(_:)), "0", [.command, .option]),
            .separator(),
            item("Enter Full Screen", #selector(NSWindow.toggleFullScreen(_:)), "f", [.command, .control], target: nil),
        ])
        add("Go", to: main, [
            item("Back", #selector(goBack(_:)), "["),
            item("Forward", #selector(goForward(_:)), "]"),
            .separator(),
            item("Previous Note", #selector(previousNote(_:)), up, [.command, .option]),
            item("Next Note", #selector(nextNote(_:)), down, [.command, .option]),
            .separator(),
            item("Follow Link", #selector(EditorTextView.followLink(_:)), "\r", target: nil),
            item("Focus Editor", #selector(focusEditor(_:)), "2"),
        ])
        let windowMenu = add("Window", to: main, [
            item("Minimize", #selector(NSWindow.performMiniaturize(_:)), "m", target: nil),
            item("Zoom", #selector(NSWindow.performZoom(_:)), "", target: nil),
        ])
        NSApp.windowsMenu = windowMenu
        return main
    }

    @discardableResult
    private func add(_ title: String, to menu: NSMenu, _ items: [NSMenuItem]) -> NSMenu {
        let holder = submenu(title, items)
        menu.addItem(holder)
        return holder.submenu!
    }

    private func submenu(_ title: String, _ items: [NSMenuItem]) -> NSMenuItem {
        let menu = NSMenu(title: title)
        items.forEach(menu.addItem)
        let holder = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        holder.submenu = menu
        return holder
    }

    private func item(
        _ title: String, _ action: Selector, _ key: String, _ modifiers: NSEvent.ModifierFlags = .command, target: AnyObject?
    ) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        item.keyEquivalentModifierMask = modifiers
        item.target = target
        return item
    }

    /// Workspace actions live on the delegate; everything else goes through the responder chain.
    private func item(_ title: String, _ action: Selector, _ key: String, _ modifiers: NSEvent.ModifierFlags = .command) -> NSMenuItem {
        item(title, action, key, modifiers, target: self)
    }

    private func find(_ title: String, _ action: NSTextFinder.Action, _ key: String, _ modifiers: NSEvent.ModifierFlags = .command) -> NSMenuItem {
        let item = item(title, #selector(NSTextView.performFindPanelAction(_:)), key, modifiers, target: nil)
        item.tag = action.rawValue
        return item
    }

    private func hidden(_ item: NSMenuItem) -> NSMenuItem {
        item.isHidden = true
        item.allowsKeyEquivalentWhenHidden = true
        return item
    }
}
