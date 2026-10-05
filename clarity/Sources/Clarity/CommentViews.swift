import SwiftUI

/// One comment in the margin, styled like a note in a printed book's margin: no box, just a rule
/// down its left edge. It brightens while the caret is in its passage, which also reveals Resolve.
struct CommentCard: View {
    let comment: Comment
    var active = false
    var onResolve: () -> Void = {}
    var onSelect: () -> Void = {}

    @State private var hovering = false

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            HStack(spacing: 6) {
                Text(comment.author).foregroundStyle(Color(active ? Theme.commentAccent : Theme.muted))
                Text(comment.created.formatted(.relative(presentation: .named))).foregroundStyle(Color(Theme.faint))
            }
            .font(Theme.ui(10))
            Text(Self.markdown(comment.body))
                .font(Theme.ui(12))
                .foregroundStyle(Color(active ? Theme.text : Theme.muted))
                .lineSpacing(3)
                .fixedSize(horizontal: false, vertical: true)
            // Always laid out, so showing it doesn't change the note's height under the cursor.
            Button(action: onResolve) {
                HStack(spacing: 8) {
                    Text("✓ Resolve").foregroundStyle(Color(Theme.commentAccent))
                    Text("⌃⌘↩").foregroundStyle(Color(Theme.faint))
                }
                .font(Theme.ui(10))
            }
            .buttonStyle(.plain)
            .opacity(active || hovering ? 1 : 0)
        }
        .marginNote(active: active)
        .contentShape(Rectangle())
        .onTapGesture(perform: onSelect)
        .onHover { hovering = $0 }
    }

    private static func markdown(_ body: String) -> AttributedString {
        (try? AttributedString(markdown: body, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(body)
    }
}

/// A comment being written: the same margin note, with a field in place of the remark.
/// Return saves, ⌥Return starts a new line, Escape (or leaving it empty) cancels.
struct DraftCommentCard: View {
    let author: String
    var onFinish: (String) -> Void
    /// The field grew or shrank, so the notes below it need to move.
    var onResize: () -> Void

    @State private var text = ""
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            Text(author)
                .font(Theme.ui(10))
                .foregroundStyle(Color(Theme.commentAccent))
            TextField("Add a comment", text: $text, axis: .vertical)
                .textFieldStyle(.plain)
                .font(Theme.ui(12))
                .foregroundStyle(Color(Theme.text))
                .lineSpacing(3)
                .focused($focused)
                .onSubmit { onFinish(text) }
                .onExitCommand { onFinish("") }
            HStack(spacing: 10) {
                Text("↩ Comment")
                Text("⌥↩ New line")
                Text("esc Cancel")
            }
            .font(Theme.ui(10))
            .foregroundStyle(Color(Theme.faint))
        }
        .marginNote(active: true)
        .onAppear { DispatchQueue.main.async { focused = true } }
        .onChange(of: text) { DispatchQueue.main.async(execute: onResize) }
        .onChange(of: focused) { _, isFocused in
            if !isFocused && text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { onFinish("") }
        }
    }
}

private extension View {
    /// A rule down the left edge, red while the comment is the one being read or written.
    func marginNote(active: Bool) -> some View {
        padding(.leading, 12)
            .padding(.vertical, 2)
            .overlay(alignment: .leading) {
                Rectangle()
                    .fill(Color(active ? Theme.commentAccent : Theme.faint).opacity(active ? 1 : 0.6))
                    .frame(width: 2)
            }
            .frame(width: EditorTextView.commentCardWidth, alignment: .leading)
    }
}
