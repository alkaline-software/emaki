import Cocoa
import WebKit
// render.swift <icon.html> <out.png>:<size>:<margin|bleed> ...
// Loads the canvas source in a web view with no window and writes what its
// exportIcon(size, bleed) returns, once per job. The canvas draws each size
// on its own pixel grid, so nothing here is a scaled-down copy of another.
let args = CommandLine.arguments
let page = URL(fileURLWithPath: args[1])
var jobs = args.dropFirst(2).map { $0.split(separator: ":").map(String.init) }

final class Renderer: NSObject, WKNavigationDelegate {
    let view = WKWebView(frame: NSRect(x: 0, y: 0, width: 1280, height: 800))
    func start() {
        view.navigationDelegate = self
        view.loadFileURL(page, allowingReadAccessTo: page.deletingLastPathComponent())
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { next() }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { fail("\(error)") }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { fail("\(error)") }
    func fail(_ why: String) -> Never {
        FileHandle.standardError.write("render.swift: \(why)\n".data(using: .utf8)!)
        exit(1)
    }
    func next() {
        guard let job = jobs.first else { exit(0) }
        jobs.removeFirst()
        let (out, size, bleed) = (job[0], job[1], job[2] == "bleed")
        view.evaluateJavaScript("exportIcon(\(size), \(bleed))") { result, error in
            guard let url = result as? String, let comma = url.firstIndex(of: ","),
                  let png = Data(base64Encoded: String(url[url.index(after: comma)...]))
            else { self.fail("\(out): \(error.map { "\($0)" } ?? "no picture")") }
            do { try png.write(to: URL(fileURLWithPath: out)) } catch { self.fail("\(out): \(error)") }
            self.next()
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let renderer = Renderer()
renderer.start()
app.run()
