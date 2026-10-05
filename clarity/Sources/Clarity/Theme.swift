import AppKit
import SwiftUI

enum Theme {
    static let background = color(dark: 0x161616, light: 0xF7F7F5)
    static let sidebar = color(dark: 0x111111, light: 0xEEEEEB)
    static let text = color(dark: 0xD4D4D4, light: 0x2A2A2A)
    static let heading = color(dark: 0xF4F4F4, light: 0x0E0E0E)
    static let muted = color(dark: 0x8A8A8A, light: 0x7C7C7C)
    /// Markdown syntax characters: present, but quieter than the words.
    static let faint = color(dark: 0x555555, light: 0xB8B8B8)
    /// Text outside the current sentence in focus mode.
    static let dimmed = color(dark: 0x3C3C3C, light: 0xCFCFCF)
    static let accent = color(dark: 0x2AA6EF, light: 0x1487D4)
    static let code = color(dark: 0x9FB4C2, light: 0x4D6272)
    static let highlight = color(dark: 0x4A4019, light: 0xFAE89A)
    static let selection = color(dark: 0x163A52, light: 0xC7E3F6)
    /// Passages with a comment on them, the one being read, and the comments' own marks.
    static let comment = color(dark: 0x3A1E21, light: 0xFCE3E3)
    static let commentActive = color(dark: 0x5E262C, light: 0xF7C4C4)
    static let commentAccent = color(dark: 0xF0575D, light: 0xD7262E)

    private static func color(dark: UInt32, light: UInt32) -> NSColor {
        NSColor(name: nil) { appearance in
            let hex = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? dark : light
            return NSColor(
                srgbRed: CGFloat(hex >> 16 & 0xFF) / 255,
                green: CGFloat(hex >> 8 & 0xFF) / 255,
                blue: CGFloat(hex & 0xFF) / 255,
                alpha: 1
            )
        }
    }

    // MARK: Fonts

    /// Registers the bundled iA Writer Mono faces. Looks in the .app's Resources first, then
    /// next to the sources so `swift run` gets the same font.
    static func registerFonts() {
        let candidates = [
            Bundle.main.resourceURL?.appending(path: "Fonts"),
            URL(filePath: #filePath).deletingLastPathComponent().appending(path: "../../Resources/Fonts").standardized,
        ]
        guard let dir = candidates.compactMap({ $0 }).first(where: { FileManager.default.fileExists(atPath: $0.path) }),
              let files = try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)
        else { return }
        for url in files where url.pathExtension == "ttf" {
            CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
        }
    }

    private static var fontCache: [String: NSFont] = [:]

    static func font(size: CGFloat, bold: Bool = false, italic: Bool = false) -> NSFont {
        let key = "\(size)-\(bold)-\(italic)"
        if let cached = fontCache[key] { return cached }
        let name = ["iAWriterMonoS-Regular", "iAWriterMonoS-Bold", "iAWriterMonoS-Italic", "iAWriterMonoS-BoldItalic"][(bold ? 1 : 0) + (italic ? 2 : 0)]
        let font = NSFont(name: name, size: size) ?? systemMono(size: size, bold: bold, italic: italic)
        fontCache[key] = font
        return font
    }

    private static func systemMono(size: CGFloat, bold: Bool, italic: Bool) -> NSFont {
        let base = NSFont.monospacedSystemFont(ofSize: size, weight: bold ? .bold : .regular)
        guard italic else { return base }
        return NSFont(descriptor: base.fontDescriptor.withSymbolicTraits(.italic), size: size) ?? base
    }

    static func ui(_ size: CGFloat) -> Font {
        Font(font(size: size) as CTFont)
    }
}

extension Color {
    init(_ color: NSColor) { self.init(nsColor: color) }
}
