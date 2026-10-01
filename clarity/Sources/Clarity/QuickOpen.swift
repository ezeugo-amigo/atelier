import SwiftUI

/// ⌘O / ⌘P: type to filter notes by name or path; Return opens the highlighted note, or creates
/// one with the typed name when nothing matches exactly.
struct QuickOpen: View {
    let workspace: Workspace
    @State private var query = ""
    @State private var selected = 0
    @FocusState private var focused: Bool

    private enum Item: Identifiable {
        case note(Note)
        case create(String)

        var id: String {
            switch self {
            case .note(let note): note.relativePath
            case .create(let name): "+" + name
            }
        }
    }

    private var items: [Item] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        guard !q.isEmpty else { return workspace.notes.prefix(50).map(Item.note) }
        let ranked = workspace.notes.enumerated()
            .compactMap { index, note in Self.score(note, q).map { (note, $0, index) } }
            .sorted { ($0.1, -$0.2) > ($1.1, -$1.2) }
            .map { Item.note($0.0) }
        let exists = workspace.notes.contains { $0.name.lowercased() == q }
        return exists ? ranked : ranked + [.create(query.trimmingCharacters(in: .whitespaces))]
    }

    private static func score(_ note: Note, _ q: String) -> Int? {
        let name = note.name.lowercased()
        if name == q { return 4 }
        if name.hasPrefix(q) { return 3 }
        if name.contains(q) { return 2 }
        var remaining = note.relativePath.lowercased()[...]
        for c in q {
            guard let i = remaining.firstIndex(of: c) else { return nil }
            remaining = remaining[remaining.index(after: i)...]
        }
        return 1
    }

    var body: some View {
        let items = items
        ZStack(alignment: .top) {
            Color.black.opacity(0.25)
                .onTapGesture(perform: close)

            VStack(spacing: 0) {
                TextField("Open or create a note", text: $query)
                    .textFieldStyle(.plain)
                    .font(Theme.ui(15))
                    .foregroundStyle(Color(Theme.heading))
                    .padding(16)
                    .focused($focused)
                    .onSubmit { choose(items) }
                    .onKeyPress(.downArrow) { move(1, items); return .handled }
                    .onKeyPress(.upArrow) { move(-1, items); return .handled }
                    .onKeyPress(.escape) { close(); return .handled }

                Rectangle().fill(Color(Theme.faint).opacity(0.4)).frame(height: 1)

                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            ForEach(Array(items.enumerated()), id: \.element.id) { index, item in
                                row(item, highlighted: index == selected)
                                    .id(index)
                                    .onTapGesture {
                                        selected = index
                                        choose(items)
                                    }
                            }
                        }
                        .padding(6)
                    }
                    .frame(maxHeight: 340)
                    .fixedSize(horizontal: false, vertical: true)
                    .onChange(of: selected) { _, index in proxy.scrollTo(index) }
                }
            }
            .frame(width: 540)
            .background(Color(Theme.sidebar))
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color(Theme.faint).opacity(0.5), lineWidth: 1))
            .shadow(color: .black.opacity(0.35), radius: 24, y: 10)
            .padding(.top, 90)
        }
        .onAppear { focused = true }
        .onChange(of: query) { selected = 0 }
    }

    @ViewBuilder
    private func row(_ item: Item, highlighted: Bool) -> some View {
        HStack(spacing: 10) {
            switch item {
            case .note(let note):
                Text(note.name).foregroundStyle(Color(Theme.text))
                Spacer()
                Text(note.folder).foregroundStyle(Color(Theme.faint)).font(Theme.ui(11))
            case .create(let name):
                Text("Create “\(name)”").foregroundStyle(Color(Theme.accent))
                Spacer()
            }
        }
        .font(Theme.ui(13))
        .lineLimit(1)
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(RoundedRectangle(cornerRadius: 6).fill(highlighted ? Color(Theme.selection) : .clear))
        .contentShape(Rectangle())
    }

    private func move(_ delta: Int, _ items: [Item]) {
        guard !items.isEmpty else { return }
        selected = min(max(selected + delta, 0), items.count - 1)
    }

    private func choose(_ items: [Item]) {
        guard items.indices.contains(selected) else { return }
        switch items[selected] {
        case .note(let note): workspace.open(note.url)
        case .create(let name): workspace.follow(name)
        }
        close()
    }

    private func close() {
        workspace.showQuickOpen = false
        workspace.focusEditor()
    }
}
