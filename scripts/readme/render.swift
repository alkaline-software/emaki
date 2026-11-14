import Cocoa
import WebKit
// render.swift <readme.html> <out dir> [first last]
// Loads the drawing in a web view with no window, waits for its faces and
// the icon (`ready`), and writes each frame `frame(i)` returns as
// <out dir>/frame-0000.png and on. The page's own playing is kept off
// (`RENDERING`), so every frame is drawn for its own moment. README_ICON
// names the app's icon, handed to the page as data (`ICON`): drawn from a
// file it would taint the canvas, and no frame could be read back.
let args = CommandLine.arguments
let page = URL(fileURLWithPath: args[1])
let out = URL(fileURLWithPath: args[2])
let root = page.deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()

final class Renderer: NSObject, WKNavigationDelegate {
    let view: WKWebView
    var i = 0, last = 0
    override init() {
        let config = WKWebViewConfiguration()
        let path = ProcessInfo.processInfo.environment["README_ICON"] ?? ""
        let icon = path.isEmpty ? "" : ((try? Data(contentsOf: URL(fileURLWithPath: path)))?.base64EncodedString() ?? "")
        config.userContentController.addUserScript(WKUserScript(source: "window.RENDERING = true; window.ICON = \(icon.isEmpty ? "''" : "'data:image/png;base64,\(icon)'");", injectionTime: .atDocumentStart, forMainFrameOnly: true))
        view = WKWebView(frame: NSRect(x: 0, y: 0, width: 1600, height: 900), configuration: config)
        super.init()
    }
    func start() {
        view.navigationDelegate = self
        view.loadFileURL(page, allowingReadAccessTo: root)
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        view.callAsyncJavaScript("await window.ready; return window.FRAMES;", arguments: [:], in: nil, in: .page) { result in
            guard case .success(let n) = result, let frames = n as? Int else { self.fail("the page is not ready: \(result)") }
            let a = CommandLine.arguments
            self.i = a.count > 4 ? Int(a[3])! : 0
            self.last = a.count > 4 ? Int(a[4])! : frames - 1
            self.next()
        }
    }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { fail("\(error)") }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { fail("\(error)") }
    func fail(_ why: String) -> Never {
        FileHandle.standardError.write("render.swift: \(why)\n".data(using: .utf8)!)
        exit(1)
    }
    func next() {
        guard i <= last else { exit(0) }
        let n = i
        view.callAsyncJavaScript("return window.frame(i);", arguments: ["i": n], in: nil, in: .page) { result in
            guard case .success(let v) = result, let url = v as? String, let comma = url.firstIndex(of: ","),
                  let png = Data(base64Encoded: String(url[url.index(after: comma)...])) else { self.fail("frame \(n): \(result)") }
            let file = out.appendingPathComponent(String(format: "frame-%04d.png", n))
            do { try png.write(to: file) } catch { self.fail("\(file.path): \(error)") }
            self.i += 1
            self.next()
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let renderer = Renderer()
renderer.start()
app.run()
