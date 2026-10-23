//! An HTML file drawn as a page: laid out with its style sheets by Blitz
//! (Stylo, Taffy and Parley, all Rust) and rastered on the CPU to
//! pictures the file's pane stacks. No webview, which draws over every
//! overlay of the window; no script is run; and nothing is fetched: the
//! one loader here reads files and answers everything else with nothing.
//!
//! It is what draws a page only where the system's web view cannot be
//! set in the window (`html_web.rs`): Linux. Elsewhere nothing here is
//! built but the shape of what it gives.

use std::sync::Arc;

/// A page as drawn: its strips top to bottom with each one's height in
/// pixels, how wide they are, the width it was laid out at, and its
/// links, each where the page has it (left, top, width, height).
pub struct Shot {
    pub strips: Vec<(Arc<gpui::Image>, u32)>,
    pub px_w: u32,
    pub css_w: f32,
    pub links: Vec<(String, [f32; 4])>,
    /// The page goes on past the last strip.
    pub more: bool,
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub use blitz::shoot;

/// Where the web view shows the page, nothing is drawn here.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn shoot(_: &str, _: &std::path::Path, _: f32, _: f32) -> Result<Shot, String> {
    Err("This page is the web view's to show.".into())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod blitz {
    use super::Shot;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use anyrender::ImageRenderer as _;
    use anyrender_vello_cpu::VelloCpuImageRenderer;
    use blitz_dom::{DocumentConfig, Point};
    use blitz_html::HtmlDocument;
    use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request, Url};
    use blitz_traits::shell::{ColorScheme, Viewport};

    /// A strip is this many pixels tall: the rasteriser's canvas has a limit
    /// a side, and a tall page as one picture is more than a texture holds.
    const STRIP: u32 = 2048;
    /// The most strips drawn; a page longer than this ends there.
    const STRIPS: u32 = 40;

    /// The one door a resource comes through. A file is read from the disk;
    /// anything else is answered with no bytes, which is an empty sheet or a
    /// picture left out. It has to be answered: a style sheet in the head
    /// that never comes holds the layout for ever.
    struct FilesOnly {
        asked: AtomicUsize,
    }

    impl NetProvider for FilesOnly {
        fn fetch(&self, _doc: usize, request: Request, handler: Box<dyn NetHandler>) {
            self.asked.fetch_add(1, Ordering::SeqCst);
            let url = request.url;
            let bytes = match url.scheme() {
                "file" => url.to_file_path().ok().and_then(|p| std::fs::read(p).ok()).unwrap_or_default(),
                _ => Vec::new(),
            };
            handler.bytes(url.to_string(), Bytes::from(bytes));
        }
    }

    /// Draw `html`, the text of the file at `file`, `width` points wide at
    /// `scale` pixels a point. Slow enough to be kept off the main thread.
    /// It is drawn light, on white, as a browser draws a page that says
    /// nothing of dark.
    pub fn shoot(html: &str, file: &Path, width: f32, scale: f32) -> Result<Shot, String> {
        let (html, file) = (html.to_string(), file.to_path_buf());
        std::panic::catch_unwind(move || draw(&html, &file, width, scale)).unwrap_or(None).ok_or_else(|| "This page could not be drawn.".to_string())
    }

    fn draw(html: &str, file: &Path, width: f32, scale: f32) -> Option<Shot> {
        let scale = scale.max(1.);
        let px_w = ((width * scale).round() as u32).clamp(64, 8192);
        let net = Arc::new(FilesOnly { asked: AtomicUsize::new(0) });
        // The height here only feeds `vh` and media queries; the page's own
        // is read back once it is laid out.
        let viewport = |h: u32| Viewport::new(px_w, h, scale, ColorScheme::Light);
        let mut doc = HtmlDocument::from_html(
            html,
            DocumentConfig { base_url: Url::from_file_path(file).ok().map(|u| u.to_string()), net_provider: Some(net.clone()), viewport: Some(viewport((800. * scale) as u32)), ..Default::default() },
        );
        // Each pass takes in what the loader answered and may ask for more
        // (an import, a background picture), so go round until nothing is.
        for _ in 0..64 {
            let before = net.asked.load(Ordering::SeqCst);
            doc.resolve(0.0);
            if net.asked.load(Ordering::SeqCst) == before && !doc.has_pending_critical_resources() {
                break;
            }
        }
        doc.resolve(0.0);
        let root = *doc.root_element().final_layout();
        let tall = (((root.size.height + root.scroll_height()).ceil() * scale).ceil() as u32).max(1);
        let px_h = tall.min(STRIP * STRIPS);

        let mut links = Vec::new();
        for id in doc.query_selector_all("a[href]").unwrap_or_default() {
            let Some(href) = doc.get_node(id).and_then(|n| n.attr(blitz_dom::local_name!("href"))).map(str::to_string) else { continue };
            for r in doc.node_client_rects(id) {
                links.push((href.clone(), [r.x as f32, r.y as f32, r.width as f32, r.height as f32]));
            }
        }

        let strip = STRIP.min(px_h);
        doc.set_viewport(viewport(strip));
        doc.resolve(0.0);
        let mut renderer = VelloCpuImageRenderer::new(px_w, strip);
        let mut rgba = vec![0u8; (px_w * strip * 4) as usize];
        let mut strips = Vec::new();
        let mut y = 0u32;
        while y < px_h {
            let rows = strip.min(px_h - y);
            doc.set_viewport_scroll(Point { x: 0.0, y: y as f64 / scale as f64 });
            rgba.fill(0);
            renderer.reset();
            renderer.render(|scene| blitz_paint::paint_scene(scene, &mut doc, scale as f64, px_w, strip, 0, 0), &mut rgba);
            // What is drawn has its color already multiplied by how solid
            // it is; laid over white it is a picture with nothing clear.
            let mut rgb = Vec::with_capacity((px_w * rows * 3) as usize);
            for s in rgba[..(px_w * rows * 4) as usize].chunks_exact(4) {
                let clear = 255 - s[3] as u32;
                rgb.extend([(s[0] as u32 + clear).min(255) as u8, (s[1] as u32 + clear).min(255) as u8, (s[2] as u32 + clear).min(255) as u8]);
            }
            let mut png = Vec::new();
            let enc = image::codecs::png::PngEncoder::new_with_quality(&mut png, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::Adaptive);
            image::ImageEncoder::write_image(enc, &rgb, px_w, rows, image::ExtendedColorType::Rgb8).ok()?;
            strips.push((Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png)), rows));
            y += rows;
        }
        Some(Shot { strips, px_w, css_w: px_w as f32 / scale, links, more: tall > px_h })
    }
}
