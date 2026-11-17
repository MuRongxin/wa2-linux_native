// White Album 2 native port - macOS launcher.
// Shows a small settings window (language / display / font / text speed),
// prepares the environment and exec()s the engine in the same process.
import AppKit
import Darwin

let fm = FileManager.default
let bundle = Bundle.main
let macosDir = bundle.executableURL!.deletingLastPathComponent()
let resDir = bundle.resourceURL!
let userDir = fm.homeDirectoryForCurrentUser
    .appendingPathComponent("Library/Application Support/WA2-native")
let iniURL = userDir.appendingPathComponent("wa2.ini")
let confURL = userDir.appendingPathComponent("launcher.conf")
try? fm.createDirectory(at: userDir, withIntermediateDirectories: true)

// ---- tiny key=value files ----
func readKV(_ url: URL) -> [(String, String)] {
    guard let s = try? String(contentsOf: url, encoding: .utf8) else { return [] }
    return s.split(separator: "\n").compactMap { line in
        let t = line.trimmingCharacters(in: .whitespaces)
        if t.isEmpty || t.hasPrefix(";") || t.hasPrefix("#") { return nil }
        guard let eq = t.firstIndex(of: "=") else { return nil }
        return (String(t[..<eq]).trimmingCharacters(in: .whitespaces),
                String(t[t.index(after: eq)...]).trimmingCharacters(in: .whitespaces))
    }
}
func writeKV(_ url: URL, _ updates: [String: String], header: String) {
    var lines: [String] = []
    var seen = Set<String>()
    if let s = try? String(contentsOf: url, encoding: .utf8) {
        for line in s.split(separator: "\n", omittingEmptySubsequences: false) {
            let t = String(line)
            if let eq = t.firstIndex(of: "="), !t.hasPrefix(";") {
                let k = String(t[..<eq]).trimmingCharacters(in: .whitespaces)
                if let v = updates[k] { lines.append("\(k)=\(v)"); seen.insert(k); continue }
            }
            lines.append(t)
        }
        while lines.last == "" { lines.removeLast() }
    } else {
        lines.append(header)
    }
    for (k, v) in updates.sorted(by: { $0.key < $1.key }) where !seen.contains(k) {
        lines.append("\(k)=\(v)")
    }
    let tmp = url.appendingPathExtension("tmp")
    try? (lines.joined(separator: "\n") + "\n").write(to: tmp, atomically: false, encoding: .utf8)
    _ = try? fm.replaceItemAt(url, withItemAt: tmp)
}

var conf = Dictionary(readKV(confURL), uniquingKeysWith: { _, b in b })
let ini = Dictionary(readKV(iniURL), uniquingKeysWith: { _, b in b })

struct FontChoice { let title: String; let env: String? }
let fonts: [FontChoice] = [
    FontChoice(title: "默认（游戏内设置）", env: nil),
    FontChoice(title: "思源黑体 Regular", env: "fonts/NotoSansCJK-Regular.ttc#2"),
    FontChoice(title: "思源黑体 Medium", env: "fonts/NotoSansCJK-Medium.ttc#2"),
    FontChoice(title: "得意黑（斜体）", env: "fonts/SmileySans-Oblique.ttf#0"),
]

func launchGame() -> Never {
    var env = ProcessInfo.processInfo.environment
    env["WA2_DATA"] = resDir.appendingPathComponent("data").path
    env["WA2_USER_DIR"] = userDir.path
    env["WA2_LANG"] = (conf["lang"] ?? "cn") == "jp" ? "jp" : "cn"
    env["WA2_ART_SCALE"] = "4"
    env["WA2_RENDER_SCALE"] = env["WA2_RENDER_SCALE"] ?? "4"
    env["WA2_VSYNC"] = env["WA2_VSYNC"] ?? "1"
    env["PATH"] = macosDir.path + ":/usr/bin:/bin"
    env.removeValue(forKey: "DYLD_LIBRARY_PATH")
    let fi = Int(conf["font"] ?? "0") ?? 0
    if fi > 0 && fi < fonts.count, let f = fonts[fi].env {
        env["WA2_FONT_GAME"] = resDir.appendingPathComponent(f).path
    } else {
        env.removeValue(forKey: "WA2_FONT_GAME")
    }
    // Engine log for troubleshooting.
    let logURL = userDir.appendingPathComponent("wa2.log")
    let fd = open(logURL.path, O_WRONLY | O_CREAT | O_TRUNC, 0o644)
    if fd >= 0 { dup2(fd, 2); dup2(fd, 1); close(fd) }
    fm.changeCurrentDirectoryPath(resDir.path)
    let exe = macosDir.appendingPathComponent("wa2-native").path
    let argv: [UnsafeMutablePointer<CChar>?] = [strdup(exe), strdup("title"), nil]
    let envp: [UnsafeMutablePointer<CChar>?] = env.map { strdup("\($0.key)=\($0.value)") } + [nil]
    execve(exe, argv, envp)
    perror("execve")
    exit(1)
}

let optionHeld = NSEvent.modifierFlags.contains(.option)
if conf["skip_dialog"] == "1" && !optionHeld {
    launchGame()
}

// ---- settings window ----
final class Delegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    var window: NSWindow!
    let lang = NSPopUpButton(), disp = NSPopUpButton(), font = NSPopUpButton(), speed = NSPopUpButton()
    let skip = NSButton(checkboxWithTitle: "下次直接启动（启动时按住 ⌥ Option 可再次打开此窗口）", target: nil, action: nil)
    var started = false

    func row(_ title: String, _ pop: NSPopUpButton, _ items: [String], _ sel: Int) -> NSStackView {
        let l = NSTextField(labelWithString: title)
        l.alignment = .right
        l.widthAnchor.constraint(equalToConstant: 90).isActive = true
        pop.addItems(withTitles: items)
        pop.selectItem(at: max(0, min(sel, items.count - 1)))
        pop.widthAnchor.constraint(equalToConstant: 260).isActive = true
        let s = NSStackView(views: [l, pop]); s.orientation = .horizontal
        return s
    }

    func applicationDidFinishLaunching(_ n: Notification) {
        let mainMenu = NSMenu()
        let appItem = NSMenuItem(); mainMenu.addItem(appItem)
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "退出 白色相簿2", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu
        NSApp.mainMenu = mainMenu

        let title = NSTextField(labelWithString: "WHITE ALBUM 2")
        title.font = .boldSystemFont(ofSize: 20)
        let sub = NSTextField(labelWithString: "Apple 芯片原生版 · 启动设置")
        sub.textColor = .secondaryLabelColor
        let langSel = (conf["lang"] ?? "cn") == "jp" ? 1 : 0
        let dispSel = Int(ini["win_disp"] ?? "1") ?? 1
        let speedSel = Int(ini["wait"] ?? "2") ?? 2
        let rows = [
            row("语言", lang, ["简体中文", "日本語"], langSel),
            row("显示模式", disp, ["窗口", "全屏（原生分辨率）"], dispSel),
            row("对话字体", font, fonts.map { $0.title }, Int(conf["font"] ?? "0") ?? 0),
            row("文字速度", speed, ["慢", "普通", "快", "瞬间"], speedSel),
        ]
        skip.state = conf["skip_dialog"] == "1" ? .on : .off
        let hint = NSTextField(wrappingLabelWithString:
            "游戏内快捷键：⌥ Option / 右键 打开菜单 · ⌘F 或 F11 切换全屏 · Esc 返回 · ⌘Q 退出\n存档与设置位于 ~/Library/Application Support/WA2-native")
        hint.font = .systemFont(ofSize: 11); hint.textColor = .secondaryLabelColor
        hint.preferredMaxLayoutWidth = 380
        let start = NSButton(title: "开始游戏", target: self, action: #selector(go))
        start.keyEquivalent = "\r"; start.bezelColor = .controlAccentColor
        let quit = NSButton(title: "退出", target: NSApp, action: #selector(NSApplication.terminate(_:)))
        let buttons = NSStackView(views: [quit, start]); buttons.orientation = .horizontal
        let stack = NSStackView(views: [title, sub] + rows + [skip, hint, buttons])
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 12
        stack.edgeInsets = NSEdgeInsets(top: 22, left: 26, bottom: 22, right: 26)
        stack.setCustomSpacing(18, after: sub)
        buttons.setHuggingPriority(.defaultLow, for: .horizontal)

        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 440, height: 380),
                          styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "白色相簿2"
        window.contentView = stack
        window.delegate = self
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    @objc func go() {
        started = true
        conf["lang"] = lang.indexOfSelectedItem == 1 ? "jp" : "cn"
        conf["font"] = String(font.indexOfSelectedItem)
        conf["skip_dialog"] = skip.state == .on ? "1" : "0"
        writeKV(confURL, conf, header: "; WA2 macOS launcher")
        writeKV(iniURL, ["win_disp": String(disp.indexOfSelectedItem),
                         "wait": String(speed.indexOfSelectedItem)],
                header: "; WHITE ALBUM 2 native port\n; keys follow the original wa2.ini")
        window.orderOut(nil)
        launchGame()
    }
    func windowWillClose(_ n: Notification) { if !started { NSApp.terminate(nil) } }
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
let delegate = Delegate()
app.delegate = delegate
app.run()
