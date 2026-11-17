// Renders a Big Sur-style 1024px app icon: rounded square with drop shadow,
// a winter background picture (argument 1) and a glowing vector snowflake.
import AppKit
import CoreGraphics

let args = CommandLine.arguments
let bgURL = URL(fileURLWithPath: args[1]), outURL = URL(fileURLWithPath: args[2])
let S: CGFloat = 1024
let cs = CGColorSpace(name: CGColorSpace.sRGB)!
let ctx = CGContext(data: nil, width: 1024, height: 1024, bitsPerComponent: 8, bytesPerRow: 0,
                    space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
let rect = CGRect(x: 100, y: 100, width: 824, height: 824)
let path = CGPath(roundedRect: rect, cornerWidth: 185, cornerHeight: 185, transform: nil)

// shadow
ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: -12), blur: 28, color: CGColor(gray: 0, alpha: 0.35))
ctx.addPath(path); ctx.setFillColor(CGColor(red: 0.85, green: 0.9, blue: 0.96, alpha: 1)); ctx.fillPath()
ctx.restoreGState()

ctx.saveGState()
ctx.addPath(path); ctx.clip()
// background picture, center-cropped to the square
if let src = CGImageSourceCreateWithURL(bgURL as CFURL, nil), let img = CGImageSourceCreateImageAtIndex(src, 0, nil) {
    let w = CGFloat(img.width), h = CGFloat(img.height)
    let side = min(w, h)
    let crop = CGRect(x: (w - side) / 2 + side * 0.02, y: (h - side) / 2, width: side, height: side)
    if let c = img.cropping(to: crop) { ctx.draw(c, in: rect) }
}
// cool winter wash: deep blue at the bottom, pale at the top
let wash = CGGradient(colorsSpace: cs, colors: [
    CGColor(red: 0.06, green: 0.16, blue: 0.36, alpha: 0.78),
    CGColor(red: 0.30, green: 0.48, blue: 0.72, alpha: 0.45),
    CGColor(red: 0.88, green: 0.94, blue: 1.00, alpha: 0.30)] as CFArray, locations: [0, 0.55, 1])!
ctx.drawLinearGradient(wash, start: CGPoint(x: 0, y: rect.minY), end: CGPoint(x: 0, y: rect.maxY), options: [])
// soft glow behind the flake
let glow = CGGradient(colorsSpace: cs, colors: [CGColor(red: 0.85, green: 0.95, blue: 1, alpha: 0.55),
                                                  CGColor(red: 0.85, green: 0.95, blue: 1, alpha: 0)] as CFArray, locations: [0, 1])!
ctx.drawRadialGradient(glow, startCenter: CGPoint(x: 512, y: 530), startRadius: 0,
                       endCenter: CGPoint(x: 512, y: 530), endRadius: 330, options: [])
// falling snow dots
var seed: UInt64 = 7
func rnd() -> CGFloat { seed = seed &* 6364136223846793005 &+ 1442695040888963407; return CGFloat(seed >> 33) / CGFloat(1 << 31) }
for _ in 0..<70 {
    let r = 2 + rnd() * 6
    ctx.setFillColor(CGColor(gray: 1, alpha: 0.35 + rnd() * 0.5))
    ctx.fillEllipse(in: CGRect(x: rect.minX + rnd() * rect.width, y: rect.minY + rnd() * rect.height, width: r, height: r))
}
// vector snowflake
func flake(_ c: CGContext, center: CGPoint, radius R: CGFloat, width lw: CGFloat) {
    c.setLineCap(.round); c.setLineJoin(.round); c.setLineWidth(lw)
    for i in 0..<6 {
        let a = CGFloat(i) * .pi / 3 + .pi / 2
        let dir = CGPoint(x: cos(a), y: sin(a))
        func p(_ t: CGFloat, _ off: CGFloat = 0, _ side: CGFloat = 1) -> CGPoint {
            let base = CGPoint(x: center.x + dir.x * R * t, y: center.y + dir.y * R * t)
            let b = a + side * .pi / 3.2
            return CGPoint(x: base.x + cos(b) * R * off, y: base.y + sin(b) * R * off)
        }
        c.move(to: p(0.08)); c.addLine(to: p(1.0))
        for (t, len) in [(0.38, 0.26), (0.62, 0.20), (0.82, 0.12)] as [(CGFloat, CGFloat)] {
            c.move(to: p(t)); c.addLine(to: p(t, len, 1))
            c.move(to: p(t)); c.addLine(to: p(t, len, -1))
        }
    }
    c.strokePath()
    // small hexagon heart
    c.setLineWidth(lw * 0.8)
    for i in 0...6 {
        let a = CGFloat(i) * .pi / 3 + .pi / 2
        let q = CGPoint(x: center.x + cos(a) * R * 0.16, y: center.y + sin(a) * R * 0.16)
        if i == 0 { c.move(to: q) } else { c.addLine(to: q) }
    }
    c.strokePath()
}
let ctr = CGPoint(x: 512, y: 530)
ctx.saveGState()
ctx.setShadow(offset: .zero, blur: 26, color: CGColor(red: 0.55, green: 0.8, blue: 1, alpha: 0.95))
ctx.setStrokeColor(CGColor(red: 0.80, green: 0.92, blue: 1, alpha: 1))
flake(ctx, center: ctr, radius: 300, width: 30)
ctx.restoreGState()
ctx.setStrokeColor(CGColor(gray: 1, alpha: 1))
flake(ctx, center: ctr, radius: 300, width: 14)
// thin inner rim highlight
ctx.restoreGState()
ctx.addPath(CGPath(roundedRect: rect.insetBy(dx: 1.5, dy: 1.5), cornerWidth: 184, cornerHeight: 184, transform: nil))
ctx.setStrokeColor(CGColor(gray: 1, alpha: 0.25)); ctx.setLineWidth(3); ctx.strokePath()

let out = ctx.makeImage()!
let dest = CGImageDestinationCreateWithURL(outURL as CFURL, "public.png" as CFString, 1, nil)!
CGImageDestinationAddImage(dest, out, nil); CGImageDestinationFinalize(dest)
