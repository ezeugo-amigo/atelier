import SwiftUI

struct ContentView: View {
    let workspace: Workspace

    var body: some View {
        HStack(spacing: 0) {
            if workspace.showSidebar && !workspace.focusMode {
                Sidebar(workspace: workspace)
                    .transition(.move(edge: .leading))
            }
            EditorView(workspace: workspace)
                .overlay(alignment: .top) { titleBar }
                .overlay(alignment: .bottomTrailing) { status }
        }
        .overlay {
            if workspace.showQuickOpen { QuickOpen(workspace: workspace) }
        }
        .background(Color(Theme.background))
        .ignoresSafeArea()
        .animation(.easeOut(duration: 0.18), value: workspace.showSidebar)
        .animation(.easeOut(duration: 0.18), value: workspace.focusMode)
    }

    private var titleBar: some View {
        Text(workspace.current?.deletingPathExtension().lastPathComponent ?? "")
            .font(Theme.ui(12))
            .foregroundStyle(Color(Theme.faint))
            .lineLimit(1)
            .frame(maxWidth: .infinity)
            .frame(height: 30)
            .background(Color(Theme.background))
            .opacity(workspace.focusMode ? 0 : 1)
            .allowsHitTesting(false)
    }

    private var status: some View {
        Text("\(workspace.wordCount.formatted()) words")
            .font(Theme.ui(11))
            .foregroundStyle(Color(Theme.faint))
            .padding(14)
            .opacity(workspace.focusMode ? 0 : 1)
            .allowsHitTesting(false)
    }
}

struct EditorView: NSViewRepresentable {
    let workspace: Workspace

    func makeNSView(context: Context) -> NSScrollView {
        let (scroll, editor) = EditorTextView.makeScrollable(fontSize: workspace.fontSize)
        editor.workspace = workspace
        editor.columnCharacters = workspace.columnWidth
        editor.focusMode = workspace.focusMode
        workspace.editor = editor
        return scroll
    }

    func updateNSView(_ nsView: NSScrollView, context: Context) {}
}
