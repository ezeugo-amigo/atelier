import SwiftUI

struct Sidebar: View {
    let workspace: Workspace

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(workspace.root.lastPathComponent.uppercased())
                .font(Theme.ui(10))
                .kerning(1.2)
                .foregroundStyle(Color(Theme.faint))
                .lineLimit(1)
                .padding(.horizontal, 18)
                .padding(.top, 46)
                .padding(.bottom, 10)

            if workspace.notes.isEmpty {
                Text("No notes yet.\n⌘N to start one.")
                    .font(Theme.ui(12))
                    .foregroundStyle(Color(Theme.muted))
                    .padding(.horizontal, 18)
            }

            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 1) {
                        ForEach(workspace.notes) { note in
                            row(note).id(note.url)
                        }
                    }
                    .padding(.horizontal, 8)
                    .padding(.bottom, 16)
                }
                .scrollIndicators(.never)
                .onChange(of: workspace.current) { _, url in
                    withAnimation { proxy.scrollTo(url) }
                }
            }
        }
        .frame(width: 240)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Color(Theme.sidebar))
    }

    private func row(_ note: Note) -> some View {
        let selected = note.url == workspace.current
        return VStack(alignment: .leading, spacing: 2) {
            Text(note.name)
                .font(Theme.ui(13))
                .foregroundStyle(Color(selected ? Theme.heading : Theme.text))
            if !note.folder.isEmpty {
                Text(note.folder)
                    .font(Theme.ui(11))
                    .foregroundStyle(Color(Theme.faint))
            }
        }
        .lineLimit(1)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(RoundedRectangle(cornerRadius: 5).fill(selected ? Color(Theme.selection) : .clear))
        .contentShape(Rectangle())
        .onTapGesture {
            workspace.open(note.url)
            workspace.focusEditor()
        }
        .contextMenu {
            Button("Rename…") { workspace.rename(note.url) }
            Button("Reveal in Finder") { workspace.reveal(note.url) }
            Divider()
            Button("Move to Trash") { workspace.trash(note.url) }
        }
    }
}
