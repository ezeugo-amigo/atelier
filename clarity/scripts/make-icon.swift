// Renders Resources/AppIcon.icns: a dark tile showing a page in focus mode — a hanging "#"
// heading, dimmed lines of text, the current line lit, and the blue caret.
//
//   swift scripts/make-icon.swift    (run from clarity/)

import AppKit

let root = URL(filePath: FileManager.default.currentDirectoryPath)
for name in ["iAWriterMonoS-Bold.ttf"] {
    CTFontManagerRegisterFontsForURL(root.appending(path: "Resources/Fonts/\(name)") as CFURL, .process, nil)
}

func rgb(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat(hex >> 16 & 0xFF) / 255, green: CGFloat(hex >> 8 & 0xFF) / 255, blue: CGFloat(hex & 0xFF) / 255, alpha: alpha)
}

/// Draws the icon on Apple's 1024pt grid: an 824pt tile centered on the canvas.
func draw(in ctx: CGContext) {
    let tile = CGRect(x: 100, y: 100, width: 824, height: 824)
    let shape = CGPath(roundedRect: tile, cornerWidth: 186, cornerHeight: 186, transform: nil)

    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -12), blur: 28, color: rgb(0x000000, 0.45))
    ctx.addPath(shape)
    ctx.setFillColor(rgb(0x161616))
    ctx.fillPath()
    ctx.restoreGState()

    ctx.saveGState()
    ctx.addPath(shape)
    ctx.clip()
    let gradient = CGGradient(colorsSpace: nil, colors: [rgb(0x2B2B2B), rgb(0x111111)] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(gradient, start: CGPoint(x: 512, y: 924), end: CGPoint(x: 512, y: 100), options: [])

    func bar(_ x: CGFloat, _ y: CGFloat, _ width: CGFloat, _ color: CGColor, height: CGFloat = 34, radius: CGFloat? = nil) {
        let r = radius ?? height / 2
        ctx.addPath(CGPath(roundedRect: CGRect(x: x, y: y, width: width, height: height), cornerWidth: r, cornerHeight: r, transform: nil))
        ctx.setFillColor(color)
        ctx.fillPath()
    }

    let left: CGFloat = 346
    let hash = NSAttributedString(string: "#", attributes: [
        .font: NSFont(name: "iAWriterMonoS-Bold", size: 132)!,
        .foregroundColor: NSColor(cgColor: rgb(0x5C5C5C))!,
    ])
    let line = CTLineCreateWithAttributedString(hash)
    ctx.textPosition = CGPoint(x: 242, y: 640)
    CTLineDraw(line, ctx)
    bar(left, 662, 330, rgb(0x3A3A3A), height: 50)

    let dim = rgb(0x323232)
    bar(left, 544, 400, dim)
    bar(left, 478, 360, dim)
    bar(left, 412, 280, rgb(0xE6E6E6))
    bar(left, 346, 380, dim)
    bar(left, 280, 240, dim)

    ctx.setShadow(offset: .zero, blur: 60, color: rgb(0x2AA6EF, 1))
    bar(left + 280 + 16, 371, 24, rgb(0x2AA6EF), height: 116, radius: 6)
    bar(left + 280 + 16, 371, 24, rgb(0x2AA6EF), height: 116, radius: 6)
    ctx.setShadow(offset: .zero, blur: 0, color: nil)

    ctx.restoreGState()

    ctx.addPath(CGPath(roundedRect: tile.insetBy(dx: 1, dy: 1), cornerWidth: 185, cornerHeight: 185, transform: nil))
    ctx.setStrokeColor(rgb(0xFFFFFF, 0.08))
    ctx.setLineWidth(2)
    ctx.strokePath()
}

func png(pixels: Int) -> Data {
    let ctx = CGContext(
        data: nil, width: pixels, height: pixels, bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )!
    ctx.scaleBy(x: CGFloat(pixels) / 1024, y: CGFloat(pixels) / 1024)
    draw(in: ctx)
    return NSBitmapImageRep(cgImage: ctx.makeImage()!).representation(using: .png, properties: [:])!
}

let iconset = FileManager.default.temporaryDirectory.appending(path: "AppIcon.iconset")
try? FileManager.default.removeItem(at: iconset)
try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
for size in [16, 32, 128, 256, 512] {
    try png(pixels: size).write(to: iconset.appending(path: "icon_\(size)x\(size).png"))
    try png(pixels: size * 2).write(to: iconset.appending(path: "icon_\(size)x\(size)@2x.png"))
}
try png(pixels: 1024).write(to: root.appending(path: "Resources/AppIcon.png"))

let iconutil = Process()
iconutil.executableURL = URL(filePath: "/usr/bin/iconutil")
iconutil.arguments = ["-c", "icns", iconset.path, "-o", root.appending(path: "Resources/AppIcon.icns").path]
try iconutil.run()
iconutil.waitUntilExit()
print(iconutil.terminationStatus == 0 ? "Wrote Resources/AppIcon.icns" : "iconutil failed")
