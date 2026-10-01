import Foundation
import Markdown

/// Renders a note as HTML for pasting into rich-text apps like Notion. Markdown can't express a
/// line break inside a table cell, so pasting a note's Markdown loses them; HTML keeps them.
///
/// Lines are rendered as they're seen in the editor: a single newline is a line break, not a space.
enum MarkdownHTML {
    static func render(_ markdown: String) -> String {
        let document = Document(parsing: stripped(markdown), options: .disableSmartOpts)
        return "<meta charset=\"utf-8\">" + blocks(document)
    }

    /// Drops frontmatter and comments, which aren't part of what a note says.
    private static func stripped(_ markdown: String) -> String {
        var text = markdown
        if text.hasPrefix("---\n"), let end = text.range(of: #"\n(---|\.\.\.)(\n|$)"#, options: .regularExpression) {
            text.removeSubrange(text.startIndex..<end.upperBound)
        }
        return MarkdownStyler.comment.stringByReplacingMatches(
            in: text, range: NSRange(location: 0, length: (text as NSString).length), withTemplate: ""
        )
    }

    private static func blocks(_ markup: Markup) -> String {
        markup.children.map(block).joined()
    }

    private static func block(_ markup: Markup) -> String {
        switch markup {
        case let heading as Heading:
            return "<h\(heading.level)>\(inlines(heading))</h\(heading.level)>"
        case let paragraph as Paragraph:
            guard paragraph.parent is ListItem else { return "<p>\(inlines(paragraph))</p>" }
            return (paragraph.indexInParent > 0 ? "<br>" : "") + inlines(paragraph)
        case let quote as BlockQuote:
            return "<blockquote>\(blocks(quote))</blockquote>"
        case let code as CodeBlock:
            let language = code.language.map { " class=\"language-\(escape($0))\"" } ?? ""
            return "<pre><code\(language)>\(escape(code.code.hasSuffix("\n") ? String(code.code.dropLast()) : code.code))</code></pre>"
        case let list as UnorderedList:
            return "<ul>\(blocks(list))</ul>"
        case let list as OrderedList:
            return "<ol\(list.startIndex == 1 ? "" : " start=\"\(list.startIndex)\"")>\(blocks(list))</ol>"
        case let item as ListItem:
            let box = item.checkbox.map { "<input type=\"checkbox\" disabled\($0 == .checked ? " checked" : "")> " } ?? ""
            return "<li>\(box)\(blocks(item))</li>"
        case let table as Table:
            return "<table>" + row(Array(table.head.cells), tag: "th", table.columnAlignments)
                + table.body.rows.map { row(Array($0.cells), tag: "td", table.columnAlignments) }.joined() + "</table>"
        case is ThematicBreak:
            return "<hr>"
        case let html as HTMLBlock:
            return html.rawHTML
        default:
            return blocks(markup)
        }
    }

    private static func row(_ cells: [Table.Cell], tag: String, _ alignments: [Table.ColumnAlignment?]) -> String {
        let html = cells.enumerated().map { column, cell in
            let align = switch column < alignments.count ? alignments[column] : nil {
            case .left: " align=\"left\""
            case .center: " align=\"center\""
            case .right: " align=\"right\""
            case nil: ""
            }
            return "<\(tag)\(align)>\(inlines(cell))</\(tag)>"
        }
        return "<tr>\(html.joined())</tr>"
    }

    /// Adjacent text nodes are rendered together: the parser splits text at brackets, which would
    /// otherwise break up wikilinks.
    private static func inlines(_ markup: Markup) -> String {
        var html = ""
        var text = ""
        for child in markup.children {
            if let t = child as? Markdown.Text {
                text += t.string
                continue
            }
            html += obsidianInlines(text) + inline(child)
            text = ""
        }
        return html + obsidianInlines(text)
    }

    private static func inline(_ markup: Markup) -> String {
        switch markup {
        case let code as InlineCode:
            return "<code>\(escape(code.code))</code>"
        case is Emphasis:
            return "<em>\(inlines(markup))</em>"
        case is Strong:
            return "<strong>\(inlines(markup))</strong>"
        case is Strikethrough:
            return "<s>\(inlines(markup))</s>"
        case let link as Link:
            return "<a href=\"\(escape(link.destination ?? ""))\">\(inlines(link))</a>"
        case let image as Image:
            return "<img src=\"\(escape(image.source ?? ""))\" alt=\"\(escape(image.plainText))\">"
        case is LineBreak, is SoftBreak:
            return "<br>"
        case let html as InlineHTML:
            return html.rawHTML
        default:
            return inlines(markup)
        }
    }

    /// Wikilinks become their alias (or target) and `==highlights==` become `<mark>`.
    private static func obsidianInlines(_ text: String) -> String {
        var html = escape(text)
        for (pattern, template) in [
            (#"!?\[\[[^\[\]\n|]+?\|([^\[\]\n]*)\]\]"#, "$1"),
            (#"!?\[\[([^\[\]\n|]+?)\]\]"#, "$1"),
            (#"==(?=\S)(.+?)(?<=\S)=="#, "<mark>$1</mark>"),
        ] {
            html = html.replacingOccurrences(of: pattern, with: template, options: .regularExpression)
        }
        return html
    }

    private static func escape(_ text: String) -> String {
        text.replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
            .replacingOccurrences(of: "\"", with: "&quot;")
    }
}
