//! A Word document in the file's pane, to read and to write in: the
//! pages as Word lays them out, a caret, a selection, typing, and a strip
//! of what is done to text and paragraphs most.
//!
//! All of it is WordCraft's (storytold/wordcraft): its engine holds the
//! document, the selection and the undo history, and every change here
//! is one of the engine's commands run by its id, the same commands its
//! own window runs. Its layout says where the caret and the selection
//! are, and its renderer draws a page. What is ours is the pane: which
//! pages are drawn, the pointer and the keys, and the strip.
//!
//! A page is a picture, drawn on the main thread when it is in view and
//! the document or the width has changed, and handed to the toolkit as
//! pixels, so the page that was there shows until the new one does. The
//! caret and the selection are drawn over it and are not part of it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::tooltip::ManagedTooltipExt as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};
use serde_json::{json, Value};
use wordcraft_engine::{EditSnapshot, Session};
use wordcraft_layout::DocLayout;

use crate::i18n::{count, phrase, t};

actions!(word, [Bold, AlignLeft, AlignCenter, AlignRight]);

/// The key context of a document being written in. The keys bound in it
/// are the ones the window binds to something else.
pub const WORD_CONTEXT: &str = "WordEdit";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-b", Bold, Some(WORD_CONTEXT)),
        KeyBinding::new("secondary-l", AlignLeft, Some(WORD_CONTEXT)),
        KeyBinding::new("secondary-e", AlignCenter, Some(WORD_CONTEXT)),
        KeyBinding::new("secondary-r", AlignRight, Some(WORD_CONTEXT)),
    ]);
}

/// Whether a file of this extension (lowercase) opens here, and whether
/// what is written in it can be saved back: WordCraft reads the binary
/// `.doc` and does not write it.
pub fn opens(ext: &str) -> Option<bool> {
    match ext {
        "docx" | "docm" | "rtf" | "odt" => Some(true),
        "doc" | "dotx" | "dotm" => Some(false),
        _ => None,
    }
}

/// Said to the window: the document has changes to save, or no longer
/// has; or something to tell the person.
pub enum WordEvent {
    Dirty,
    Said(String),
}

/// Room around the pages and between them, in points of the window.
const PAD: f32 = 18.;
const GAP: f32 = 12.;
/// A page is drawn again for a new width once the width has stood still
/// this long; until then the one there is stretched.
const STILL: Duration = Duration::from_millis(200);
const BLINK: Duration = Duration::from_millis(530);
const MENU: Duration = Duration::from_millis(130);

struct Drawn {
    rev: u64,
    px_w: u32,
    image: Arc<RenderImage>,
}

#[derive(Clone, Copy, PartialEq)]
enum Pick {
    Style,
    Size,
}

pub struct WordPane {
    pub path: PathBuf,
    /// The file's time when it was read or last saved here.
    pub mtime: Option<std::time::SystemTime>,
    writes: bool,
    session: Session,
    focus: FocusHandle,
    scroll: ScrollHandle,
    drawn: HashMap<usize, Drawn>,
    /// Where each page is in the window, as last laid out.
    places: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    /// How wide the pages' room is.
    room: Rc<Cell<f32>>,
    width_at: (f32, Instant),
    dragging: bool,
    /// What an input method is still composing, and the document as it
    /// was before any of it went in.
    composing: Option<(EditSnapshot, String)>,
    blink_at: Instant,
    was_dirty: bool,
    pick: Option<(Pick, bool, Instant)>,
}

impl EventEmitter<WordEvent> for WordPane {}

impl Focusable for WordPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// A document read from the disk, off the main thread.
pub fn read(path: &Path) -> Result<wordcraft_doc::Document, String> {
    let path = path.to_path_buf();
    std::panic::catch_unwind(move || wordcraft_engine::io::open_path(&path)).unwrap_or_else(|_| Err(t("This document could not be read.").into()))
}

impl WordPane {
    pub fn new(path: &Path, doc: wordcraft_doc::Document, cx: &mut Context<Self>) -> Self {
        let mut session = Session::new(doc);
        session.path = Some(path.to_path_buf());
        session.dirty = false;
        // The caret blinks while the pane has the keys.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(BLINK).await;
            if this.update(cx, |_, cx| cx.notify()).is_err() {
                break;
            }
        })
        .detach();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        WordPane {
            path: path.to_path_buf(),
            mtime: std::fs::metadata(path).ok().and_then(|m| m.modified().ok()),
            writes: opens(&ext).unwrap_or(false),
            session,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            drawn: HashMap::new(),
            places: Default::default(),
            room: Rc::new(Cell::new(0.)),
            width_at: (0., Instant::now()),
            dragging: false,
            composing: None,
            blink_at: Instant::now(),
            was_dirty: false,
            pick: None,
        }
    }

    pub fn dirty(&self) -> bool {
        self.session.dirty
    }

    /// The document as the disk now has it, in place of the one here.
    pub fn reload(&mut self, doc: wordcraft_doc::Document, cx: &mut Context<Self>) {
        self.session.set_document(doc);
        self.session.dirty = false;
        self.composing = None;
        self.mtime = std::fs::metadata(&self.path).ok().and_then(|m| m.modified().ok());
        self.after(cx);
    }

    /// Written to its file. What went wrong, when something did.
    pub fn save(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        if !self.writes {
            return Err(t("this kind of file is read and not written; it is not saved").into());
        }
        self.session.run("file.save", &json!({})).map_err(|e| e.to_string())?;
        self.mtime = std::fs::metadata(&self.path).ok().and_then(|m| m.modified().ok());
        self.after(cx);
        Ok(())
    }

    /// A probe's step (`docs/probing.md`).
    pub fn probe(&mut self, step: &str, cx: &mut Context<Self>) {
        match step.split_once(':') {
            Some(("type", words)) => self.typed(words, false, cx),
            Some(("compose", words)) => self.typed(words, true, cx),
            Some(("run", id)) => {
                self.run(id, json!({}), cx);
            }
            Some(("style", name)) => {
                self.run("para.style", json!({"style": name}), cx);
            }
            _ => match step {
                "all" => {
                    self.run("select.all", json!({}), cx);
                }
                "style" => self.open_pick(Pick::Style, cx),
                _ => {}
            },
        }
    }

    /// One of the engine's commands, by its id.
    fn run(&mut self, id: &str, params: Value, cx: &mut Context<Self>) -> bool {
        let changes = self.session.registry.get(id).is_some_and(|spec| spec.mutates);
        if changes && !self.writes {
            cx.emit(WordEvent::Said(t("This kind of file is shown and not written in. Save it as .docx in Word to change it.").into()));
            return false;
        }
        let done = self.session.run(id, &params);
        self.after(cx);
        done.is_ok()
    }

    /// After anything that may have moved the caret or changed the
    /// document: the caret is lit and in view.
    fn after(&mut self, cx: &mut Context<Self>) {
        self.blink_at = Instant::now();
        self.caret_in_view();
        if self.session.dirty != self.was_dirty {
            self.was_dirty = self.session.dirty;
            cx.emit(WordEvent::Dirty);
        }
        cx.notify();
    }

    /// How many window points a point of the page is.
    fn zoom(&self, layout: &DocLayout) -> f32 {
        let widest = layout.pages.iter().map(|p| p.w).fold(1., f32::max);
        (self.room.get() - 2. * PAD).max(120.) / widest
    }

    /// Each page's top in the scrolled column.
    fn tops(layout: &DocLayout, zoom: f32) -> Vec<f32> {
        let mut y = PAD;
        layout
            .pages
            .iter()
            .map(|p| {
                let top = y;
                y += p.h * zoom + GAP;
                top
            })
            .collect()
    }

    fn caret_in_view(&mut self) {
        let layout = self.session.layout();
        let Some(caret) = layout.caret_on(&self.session.sel.focus, self.session.page_hint) else { return };
        let zoom = self.zoom(&layout);
        let Some(top) = Self::tops(&layout, zoom).get(caret.page).copied() else { return };
        let (y0, y1) = (top + caret.top * zoom, top + (caret.top + caret.height) * zoom);
        let view = f32::from(self.scroll.bounds().size.height);
        if view <= 0. {
            return;
        }
        let mut off = self.scroll.offset();
        let at = -f32::from(off.y);
        let margin = 24f32.min(view / 4.);
        if y0 - margin < at {
            off.y = px(-(y0 - margin).max(0.));
        } else if y1 + margin > at + view {
            off.y = px(-(y1 + margin - view));
        } else {
            return;
        }
        self.scroll.set_offset(off);
    }

    /// The page under a place in the window, or the nearest, and the
    /// place in the page's own points.
    fn page_at(&mut self, at: Point<Pixels>) -> Option<(usize, f32, f32)> {
        let layout = self.session.layout();
        let zoom = self.zoom(&layout);
        let places = self.places.borrow();
        let (ix, b) = places.iter().enumerate().filter(|(_, b)| b.size.height > px(0.)).min_by(|(_, a), (_, b)| {
            let far = |b: &Bounds<Pixels>| if at.y < b.top() { f32::from(b.top() - at.y) } else if at.y > b.bottom() { f32::from(at.y - b.bottom()) } else { 0. };
            far(a).total_cmp(&far(b))
        })?;
        let page = layout.pages.get(ix)?;
        let x = (f32::from(at.x - b.left()) / zoom).clamp(0., page.w);
        let y = (f32::from(at.y - b.top()) / zoom).clamp(0., page.h);
        Some((ix, x, y))
    }

    fn press(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.close_pick(cx);
        let Some((page, x, y)) = self.page_at(e.position) else { return };
        self.session.close_typing();
        self.session.page_hint = page;
        match e.click_count {
            2 => {
                self.run("caret.set", json!({"page": page, "x": x, "y": y}), cx);
                self.run("select.word", json!({}), cx);
            }
            n if n >= 3 => {
                self.run("select.paragraph", json!({}), cx);
            }
            _ => {
                self.run("caret.set", json!({"page": page, "x": x, "y": y, "extend": e.modifiers.shift}), cx);
                self.dragging = true;
            }
        }
    }

    fn drag(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        if e.pressed_button != Some(MouseButton::Left) {
            self.dragging = false;
            return;
        }
        if let Some((page, x, y)) = self.page_at(e.position) {
            self.session.page_hint = page;
            self.run("caret.set", json!({"page": page, "x": x, "y": y, "extend": true}), cx);
        }
    }

    fn copy(&mut self, cut: bool, cx: &mut Context<Self>) {
        if self.session.sel.is_collapsed() {
            return;
        }
        if self.run(if cut && self.writes { "edit.cut" } else { "edit.copy" }, json!({}), cx) {
            cx.write_to_clipboard(ClipboardItem::new_string(self.session.clipboard_text.clone()));
        }
    }

    /// What the system holds is pasted; when it is what was last copied
    /// here, with the formatting it was copied with.
    fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else { return };
        let ours = !text.is_empty() && text == self.session.clipboard_text;
        self.run("edit.paste", if ours { json!({}) } else { json!({"text": text}) }, cx);
    }

    fn key(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (k, m) = (&e.keystroke, e.keystroke.modifiers);
        let cmd = m.secondary();
        let name = match k.key.as_str() {
            "left" => "Left",
            "right" => "Right",
            "up" => "Up",
            "down" => "Down",
            "home" => "Home",
            "end" => "End",
            "pageup" => "PageUp",
            "pagedown" => "PageDown",
            "enter" => "Enter",
            "tab" => "Tab",
            "backspace" => "Backspace",
            "delete" => "Delete",
            "escape" => "Escape",
            other => other,
        };
        let named = name.len() > 1 && name != "space";
        // A letter with nothing held, or with what makes another letter
        // of it, is the input method's to type.
        if !named && !cmd && !(m.control && !cfg!(target_os = "macos")) {
            return;
        }
        if name == "Escape" && self.pick.is_some_and(|(_, open, _)| open) {
            self.close_pick(cx);
            cx.stop_propagation();
            return;
        }
        let extend = json!({"extend": m.shift});
        // A Mac moves by ⌘ to a line's ends and the document's, and by
        // ⌥ over words; the engine's own keys are Windows'.
        let mac: Option<(&str, Value)> = if !cfg!(target_os = "macos") {
            None
        } else {
            match (name, m.platform, m.alt) {
                ("Left", true, _) => Some(("caret.home", extend.clone())),
                ("Right", true, _) => Some(("caret.end", extend.clone())),
                ("Up", true, _) => Some(("caret.docStart", extend.clone())),
                ("Down", true, _) => Some(("caret.docEnd", extend.clone())),
                ("Left", _, true) => Some(("caret.wordLeft", extend.clone())),
                ("Right", _, true) => Some(("caret.wordRight", extend.clone())),
                ("Up", _, true) => Some(("caret.paraUp", extend.clone())),
                ("Down", _, true) => Some(("caret.paraDown", extend.clone())),
                ("Backspace", true, _) => {
                    if self.session.sel.is_collapsed() {
                        self.run("caret.home", json!({"extend": true}), cx);
                    }
                    Some(("text.backspace", json!({})))
                }
                _ => None,
            }
        };
        if let Some((id, params)) = mac {
            self.run(id, params, cx);
            cx.stop_propagation();
            return;
        }
        match (cmd, name) {
            (true, "c") => self.copy(false, cx),
            (true, "x") => self.copy(true, cx),
            (true, "v") if !m.shift && !m.alt => self.paste(cx),
            // Saving, finding and the window's own keys are the window's.
            (true, "s" | "f" | "w" | "t" | "n" | "o" | "p" | "q" | "k" | ",") => return,
            _ => {
                let mut key = String::new();
                for (on, word) in [(cmd, "Mod+"), (m.control && cfg!(target_os = "macos"), "Ctrl+"), (m.alt, "Alt+")] {
                    if on {
                        key.push_str(word);
                    }
                }
                let last = if named { name.to_string() } else { name.to_uppercase() };
                let keys = wordcraft_engine::keymap::KeyMap::default();
                let reg = self.session.registry.clone();
                let whole = if m.shift { format!("{key}Shift+{last}") } else { format!("{key}{last}") };
                let found = keys.resolve(&reg, &whole).map(|spec| (spec.id, json!({}))).or_else(|| {
                    // A caret's key with ⇧ held is the same move, selecting.
                    keys.resolve(&reg, &format!("{key}{last}")).filter(|spec| m.shift && spec.id.starts_with("caret.")).map(|spec| (spec.id, extend.clone()))
                });
                let Some((id, params)) = found else { return };
                // What opens one of WordCraft's own dialogs has nothing
                // to open here.
                if id.ends_with("Dialog") || id.starts_with("file.") || id.starts_with("view.") || id.starts_with("window.") {
                    return;
                }
                self.run(id, params, cx);
            }
        }
        cx.stop_propagation();
    }

    /// What an input method gives: its composing letters, which stand in
    /// the document until it has decided, or the text it decided on.
    fn typed(&mut self, text: &str, composing: bool, cx: &mut Context<Self>) {
        if !self.writes {
            if !text.is_empty() {
                cx.emit(WordEvent::Said(t("This kind of file is shown and not written in. Save it as .docx in Word to change it.").into()));
            }
            return;
        }
        if let Some((before, _)) = self.composing.take() {
            self.session.restore(before);
        }
        if !text.is_empty() {
            if composing {
                self.composing = Some((self.session.edit_snapshot(), text.to_string()));
                self.session.close_typing();
            }
            let _ = self.session.run("text.insert", &json!({"text": text}));
        }
        self.after(cx);
    }

    fn open_pick(&mut self, which: Pick, cx: &mut Context<Self>) {
        self.pick = match self.pick {
            Some((was, true, _)) if was == which => Some((which, false, Instant::now())),
            _ => Some((which, true, Instant::now())),
        };
        self.pick_tick(cx);
    }

    fn close_pick(&mut self, cx: &mut Context<Self>) {
        if let Some((which, true, _)) = self.pick {
            self.pick = Some((which, false, Instant::now()));
            self.pick_tick(cx);
        }
    }

    /// The list goes once it has faded.
    fn pick_tick(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(MENU + Duration::from_millis(20)).await;
            let _ = this.update(cx, |this, cx| {
                if this.pick.is_some_and(|(_, open, at)| !open && at.elapsed() >= MENU) {
                    this.pick = None;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The pictures of the pages in view, drawn where the document or
    /// the width is no longer what they were drawn for.
    fn draw_pages(&mut self, layout: &DocLayout, zoom: f32, window: &mut Window, cx: &mut Context<Self>) {
        let room = self.room.get();
        if (room - self.width_at.0).abs() > 0.5 {
            self.width_at = (room, Instant::now());
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(STILL + Duration::from_millis(20)).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        let still = self.width_at.1.elapsed() >= STILL;
        let rev = self.session.rev();
        let scale = window.scale_factor();
        let view = f32::from(self.scroll.bounds().size.height).max(600.);
        let at = -f32::from(self.scroll.offset().y);
        let mut how = wordcraft_render::RenderOptions::default();
        how.display.dim_header = false;
        for (ix, top) in Self::tops(layout, zoom).into_iter().enumerate() {
            let page = &layout.pages[ix];
            let tall = page.h * zoom;
            if top + tall < at - view * 0.5 || top > at + view * 1.5 {
                continue;
            }
            let px_w = ((page.w * zoom * scale).round() as u32).clamp(64, 4096);
            let stale = match self.drawn.get(&ix) {
                None => true,
                Some(was) => was.rev != rev || (was.px_w != px_w && still),
            };
            if !stale {
                continue;
            }
            let shot = wordcraft_render::render_page(&self.session.doc, page, px_w as f32 / page.w.max(1.), &how);
            let mut pixels = shot.pixels;
            // The toolkit's pixels are blue first.
            for p in pixels.chunks_exact_mut(4) {
                p.swap(0, 2);
            }
            let Some(buffer) = image::RgbaImage::from_raw(shot.width, shot.height, pixels) else { continue };
            let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
            if let Some(old) = self.drawn.insert(ix, Drawn { rev, px_w: shot.width, image }) {
                cx.drop_image(old.image, Some(window));
            }
        }
        // Pages the document no longer has.
        let gone: Vec<usize> = self.drawn.keys().filter(|ix| **ix >= layout.pages.len()).copied().collect();
        for ix in gone {
            if let Some(old) = self.drawn.remove(&ix) {
                cx.drop_image(old.image, Some(window));
            }
        }
    }

    fn strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let state = wordcraft_engine::cmd::format::state(&self.session);
        let on = |key: &str| state.get(key).and_then(Value::as_bool).unwrap_or(false);
        let align = self.session.doc.para_at(&self.session.sel.focus).and_then(|p| p.props.align);
        let lit = theme.primary.opacity(0.16);
        let button = |id: &'static str, tip: &'static str, lit_now: bool, inner: AnyElement, run: (&'static str, Value)| {
            div()
                .id(id)
                .flex_shrink_0()
                .h(px(24.))
                .min_w(px(24.))
                .px(px(4.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .text_size(px(13.))
                .text_color(theme.foreground)
                .when(lit_now, |d| d.bg(lit))
                .hover(|d| d.bg(theme.muted))
                .cursor_pointer()
                .child(inner)
                .managed_tooltip(move |window, cx| crate::workbench::tip(t(tip), window, cx))
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_click(cx.listener(move |this, _, window, cx| {
                    crate::workbench::swallow_click(window, cx);
                    this.close_pick(cx);
                    this.run(run.0, run.1.clone(), cx);
                    window.focus(&this.focus, cx);
                }))
        };
        let icon = |name: &str| Icon::default().path(SharedString::from(format!("icons/{name}.svg"))).with_size(px(15.)).into_any_element();
        let sep = || div().flex_shrink_0().w(px(1.)).h(px(16.)).mx(px(4.)).bg(theme.border);
        let chosen = |id: &'static str, words: String, w: f32, which: Pick| {
            h_flex()
                .id(id)
                .flex_shrink_0()
                .h(px(24.))
                .w(px(w))
                .px(px(8.))
                .gap(px(4.))
                .rounded(px(5.))
                .border_1()
                .border_color(theme.border)
                .text_size(px(12.))
                .text_color(theme.foreground)
                .hover(|d| d.bg(theme.muted))
                .cursor_pointer()
                .child(div().flex_1().min_w_0().truncate().child(words))
                .child(Icon::default().path("icons/chevron-down.svg").with_size(px(11.)).text_color(theme.muted_foreground))
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_click(cx.listener(move |this, _, window, cx| {
                    crate::workbench::swallow_click(window, cx);
                    this.open_pick(which, cx);
                }))
        };
        let style = state.get("styleName").and_then(Value::as_str).unwrap_or("Normal").to_string();
        let size = state.get("size").and_then(Value::as_f64).map(|s| if s.fract() == 0. { format!("{s:.0}") } else { format!("{s:.1}") }).unwrap_or_default();
        let letter = |words: &'static str| div().font_family("Times New Roman").text_size(px(15.)).child(words);
        h_flex()
            .flex_shrink_0()
            .w_full()
            .min_h(px(36.))
            .px(px(10.))
            .py(px(5.))
            .gap(px(2.))
            // In a narrow pane the strip goes on to a second line.
            .flex_wrap()
            .border_b_1()
            .border_color(theme.border)
            .child(button("word-undo", phrase("Undo (⌘Z)"), false, icon("undo"), ("edit.undo", json!({}))).when(!self.session.can_undo(), |d| d.opacity(0.4)))
            .child(button("word-redo", phrase("Redo (⌘⇧Z)"), false, icon("redo"), ("edit.redo", json!({}))).when(!self.session.can_redo(), |d| d.opacity(0.4)))
            .child(sep())
            .child(chosen("word-style", style, 132., Pick::Style))
            .child(div().w(px(4.)))
            .child(chosen("word-size", size, 58., Pick::Size))
            .child(sep())
            .child(button("word-bold", phrase("Bold (⌘B)"), on("bold"), letter("B").font_weight(FontWeight::BOLD).into_any_element(), ("format.bold", json!({}))))
            .child(button("word-italic", phrase("Italic (⌘I)"), on("italic"), letter("I").italic().into_any_element(), ("format.italic", json!({}))))
            .child(button("word-underline", phrase("Underline (⌘U)"), on("underline"), letter("U").underline().into_any_element(), ("format.underline", json!({}))))
            .child(button("word-strike", phrase("Strikethrough"), on("strike"), letter("S").line_through().into_any_element(), ("format.strikethrough", json!({}))))
            .child(sep())
            .child(button("word-left", phrase("Align left (⌘L)"), matches!(align, None | Some(wordcraft_doc::props::Align::Left)), icon("align-left"), ("para.align", json!({"value": "left"}))))
            .child(button("word-center", phrase("Center (⌘E)"), align == Some(wordcraft_doc::props::Align::Center), icon("align-center"), ("para.align", json!({"value": "center"}))))
            .child(button("word-right", phrase("Align right (⌘R)"), align == Some(wordcraft_doc::props::Align::Right), icon("align-right"), ("para.align", json!({"value": "right"}))))
            .child(button("word-justify", phrase("Justify (⌘J)"), align == Some(wordcraft_doc::props::Align::Justify), icon("align-justified"), ("para.align", json!({"value": "justify"}))))
            .child(sep())
            .child(button("word-bullets", phrase("Bullets"), false, icon("list-bullets"), ("para.bullets", json!({}))))
            .child(button("word-numbers", phrase("Numbering"), false, icon("list-numbers"), ("para.numbering", json!({}))))
            .child(button("word-outdent", phrase("Decrease indent"), false, icon("indent-decrease"), ("para.outdent", json!({}))))
            .child(button("word-indent", phrase("Increase indent"), false, icon("indent-increase"), ("para.indent", json!({}))))
            .into_any_element()
    }

    /// The list under the style or the size, coming in and going out
    /// over a moment.
    fn pick_list(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (which, open, since) = self.pick?;
        let theme = cx.theme().clone();
        let state = wordcraft_engine::cmd::format::state(&self.session);
        let rows: Vec<(String, &'static str, Value, bool)> = match which {
            Pick::Style => {
                let now = state.get("style").and_then(Value::as_str).unwrap_or("Normal").to_string();
                self.session.doc.styles.gallery().into_iter().filter(|st| matches!(st.kind, wordcraft_doc::styles::StyleKind::Paragraph)).map(|st| (st.name.clone(), "para.style", json!({"style": st.id}), st.id == now)).collect()
            }
            Pick::Size => {
                let now = state.get("size").and_then(Value::as_f64);
                [8., 9., 10., 10.5, 11., 12., 14., 16., 18., 20., 24., 28., 36., 48., 72.].into_iter().map(|s: f64| (if s.fract() == 0. { format!("{s:.0}") } else { format!("{s:.1}") }, "format.size", json!({"size": s}), now == Some(s))).collect()
            }
        };
        let (left, wide) = match which {
            Pick::Style => (72., 200.),
            Pick::Size => (212., 72.),
        };
        // The rows scroll inside the card's room above and below them, so
        // one cut at the edge stays inside its corner: a clip is a rectangle.
        let list = v_flex().id("word-pick-rows").max_h(px(312.)).overflow_y_scroll().px(px(4.)).children(rows.into_iter().enumerate().map(|(ix, (words, id, params, here))| {
            div()
                .id(("word-pick-row", ix))
                .h(px(26.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .when(here, |d| d.bg(theme.primary.opacity(0.16)))
                .hover(|d| d.bg(theme.muted))
                .cursor_pointer()
                .child(div().truncate().child(words))
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_click(cx.listener(move |this, _, window, cx| {
                    crate::workbench::swallow_click(window, cx);
                    this.close_pick(cx);
                    this.run(id, params.clone(), cx);
                    window.focus(&this.focus, cx);
                }))
        }));
        let list = v_flex().id("word-pick").occlude().w(px(wide)).py(px(4.)).rounded(px(8.)).bg(theme.popover).border_1().border_color(theme.border).shadow_lg().text_size(px(12.5)).text_color(theme.popover_foreground).child(list);
        let t = (since.elapsed().as_secs_f32() / MENU.as_secs_f32()).min(1.);
        let t = if open { t } else { 1. - t };
        if t < 1. {
            cx.notify();
        }
        Some(deferred(div().absolute().top(px(34. - 4. * (1. - t))).left(px(left)).opacity(t).child(list)).with_priority(1).into_any_element())
    }
}

impl Render for WordPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let layout = self.session.layout();
        let zoom = self.zoom(&layout);
        self.draw_pages(&layout, zoom, window, cx);
        let focused = self.focus.is_focused(window);
        let lit = focused && (self.blink_at.elapsed().as_millis() / BLINK.as_millis()) % 2 == 0;
        let (a, b) = self.session.sel.ordered();
        let marks: Rc<Vec<(usize, wordcraft_geom::Rect)>> = Rc::new(if self.session.sel.is_collapsed() { Vec::new() } else { layout.selection_rects(&self.session.doc, &a, &b, self.session.page_hint) });
        let caret = layout.caret_on(&self.session.sel.focus, self.session.page_hint);
        let (entity, focus) = (cx.entity(), self.focus.clone());
        let pages = layout.pages.iter().enumerate().map(|(ix, page)| {
            let (w, h) = (page.w * zoom, page.h * zoom);
            let (places, marks, entity, focus) = (self.places.clone(), marks.clone(), entity.clone(), focus.clone());
            let caret = caret.as_ref().filter(|c| c.page == ix).map(|c| (c.x, c.top, c.height));
            let count = layout.pages.len();
            div()
                .id(("word-page", ix))
                .relative()
                .w(px(w))
                .h(px(h))
                .flex_shrink_0()
                .bg(gpui::white())
                .border_1()
                .border_color(theme.border)
                .cursor(CursorStyle::IBeam)
                .children(self.drawn.get(&ix).map(|d| img(ImageSource::Render(d.image.clone())).absolute().top_0().left_0().size_full()))
                .child(
                    canvas(
                        move |bounds, _, _| {
                            let mut places = places.borrow_mut();
                            places.resize(count, Bounds::default());
                            places[ix] = bounds;
                        },
                        move |bounds, _, window, cx| {
                            let at = |r: &wordcraft_geom::Rect| Bounds::new(bounds.origin + point(px(r.x * zoom), px(r.y * zoom)), size(px(r.w * zoom), px(r.h * zoom)));
                            for (_, r) in marks.iter().filter(|(page, _)| *page == ix) {
                                window.paint_quad(fill(at(r), gpui::rgba(0x2f7cf650)));
                            }
                            if let Some((x, top, tall)) = caret {
                                let spot = Bounds::new(bounds.origin + point(px(x * zoom - 0.75), px(top * zoom)), size(px(1.5), px(tall * zoom)));
                                // The input method's list goes where the caret is.
                                window.handle_input(&focus, ElementInputHandler::new(spot, entity.clone()), cx);
                                if lit {
                                    window.paint_quad(fill(spot, gpui::black()));
                                }
                            }
                        },
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
                .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, window, cx| this.press(e, window, cx)))
        });
        let pages: Vec<_> = pages.collect();
        let room = self.room.clone();
        let words = self.session.word_count();
        let page_count = layout.pages.len();
        let foot = h_flex().flex_shrink_0().h(px(24.)).px(px(12.)).gap(px(12.)).border_t_1().border_color(theme.border).text_size(px(11.)).text_color(theme.muted_foreground).child(count(page_count, "{n} page", "{n} pages")).child(count(words, "{n} word", "{n} words")).when(!self.writes, |d| d.child(t("Shown, not written in")));
        v_flex()
            .id("word-pane")
            .key_context(WORD_CONTEXT)
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .on_key_down(cx.listener(Self::key))
            .on_action(cx.listener(|this, _: &Bold, _, cx| {
                this.run("format.bold", json!({}), cx);
            }))
            .on_action(cx.listener(|this, _: &AlignLeft, _, cx| {
                this.run("para.align", json!({"value": "left"}), cx);
            }))
            .on_action(cx.listener(|this, _: &AlignCenter, _, cx| {
                this.run("para.align", json!({"value": "center"}), cx);
            }))
            .on_action(cx.listener(|this, _: &AlignRight, _, cx| {
                this.run("para.align", json!({"value": "right"}), cx);
            }))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| this.drag(e, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.dragging = false))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| this.dragging = false))
            .child(self.strip(cx))
            .child(
                div().relative().flex_1().min_h_0().child(
                    div()
                        .id("word-pages")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify()))
                        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                            window.focus(&this.focus, cx);
                            this.close_pick(cx);
                        }))
                        .child(canvas(move |bounds, _, _| room.set(f32::from(bounds.size.width)), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
                        .child(v_flex().w_full().items_center().py(px(PAD)).gap(px(GAP)).children(pages)),
                )
                .vertical_scrollbar(&self.scroll),
            )
            .child(foot)
            .children(self.pick_list(cx))
    }
}

impl EntityInputHandler for WordPane {
    fn text_for_range(&mut self, range: std::ops::Range<usize>, _: &mut Option<std::ops::Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let units: Vec<u16> = self.composing.as_ref().map(|(_, text)| text.encode_utf16().collect()).unwrap_or_default();
        Some(String::from_utf16_lossy(units.get(range.start.min(units.len())..range.end.min(units.len())).unwrap_or(&[])))
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let at = self.composing.as_ref().map(|(_, text)| text.encode_utf16().count()).unwrap_or(0);
        Some(UTF16Selection { range: at..at, reversed: false })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<std::ops::Range<usize>> {
        self.composing.as_ref().map(|(_, text)| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.composing = None;
    }

    fn replace_text_in_range(&mut self, _: Option<std::ops::Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        self.typed(text, false, cx);
    }

    fn replace_and_mark_text_in_range(&mut self, _: Option<std::ops::Range<usize>>, text: &str, _: Option<std::ops::Range<usize>>, _: &mut Window, cx: &mut Context<Self>) {
        self.typed(text, true, cx);
    }

    fn bounds_for_range(&mut self, _: std::ops::Range<usize>, spot: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        Some(spot)
    }

    fn character_index_for_point(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }
}
