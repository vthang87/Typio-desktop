// Renders an SVG to a square PNG with a transparent background:
//   swift render-svg.swift in.svg out.png 1024 [gray]
// `gray` draws it grey and faded, for the tray icon while Typio is off.
import AppKit

let arguments = CommandLine.arguments
guard arguments.count == 4 || arguments.count == 5, let size = Int(arguments[3]),
      let image = NSImage(contentsOfFile: arguments[1]) else {
    FileHandle.standardError.write("usage: render-svg.swift in.svg out.png size [gray]\n".data(using: .utf8)!)
    exit(1)
}
let bitmap = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size, bitsPerSample: 8,
    samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
    bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
image.draw(in: NSRect(x: 0, y: 0, width: size, height: size))
NSGraphicsContext.restoreGraphicsState()
if arguments.count == 5 && arguments[4] == "gray", let data = bitmap.bitmapData {
    // Premultiplied RGBA: grey by luminance, then fade every channel alike.
    for offset in stride(from: 0, to: bitmap.bytesPerRow * size, by: 4) {
        let r = Double(data[offset]), g = Double(data[offset + 1]), b = Double(data[offset + 2])
        let alpha = Double(data[offset + 3])
        let grey = (0.299 * r + 0.587 * g + 0.114 * b) * 0.55
        data[offset] = UInt8(grey)
        data[offset + 1] = UInt8(grey)
        data[offset + 2] = UInt8(grey)
        data[offset + 3] = UInt8(alpha * 0.55)
    }
}
try! bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: arguments[2]))
