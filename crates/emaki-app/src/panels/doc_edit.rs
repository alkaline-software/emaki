//! A markdown or Quarto file written in as it reads (`docs/panels.md`,
//! Writing in a file as it reads).
//!
//! The whole file is one editor, the composer's (`live/`, Onyx's canvas),
//! kept for as long as the file shows: the text drawn as it reads, the
//! marks shown where the caret or the selection is. What the editor does
//! not draw (a picture, a formula, a callout, a chunk, the YAML header's
//! band) the reading view draws over its lines (`live::Piece`), as Onyx
//! draws a picture or a file's frame over its row. What is typed goes at
//! once into the file's editor, so saving and the dot are the editor's
//! either way.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

use crate::live::{self, EditorEvent};
use emaki_core::doc_edit::{self as text, Map};
use gpui_component::text::{KeptViews, TextViewState};

use super::*;

/// The longest file written in as it reads.
pub(super) const DOC_MOST: usize = 256 * 1024;

/// The room around a markdown file as it reads, across and down.
pub(super) const DOC_PAD: (f32, f32) = (18., 16.);

/// The file showing as one editor.
pub(crate) struct DocCanvas {
    pub(crate) path: PathBuf,
    pub(crate) editor: Entity<live::Editor>,
    /// The text last handed to the file's editor or taken from it.
    held: String,
    /// The reading view's state for each piece, by the piece's key.
    states: HashMap<u64, Entity<TextViewState>>,
    kept: KeptViews,
    /// What the pieces were made from: the editor's count of readings and
    /// the look.
    made: Option<(u64, u64)>,
    _subs: Vec<Subscription>,
}

impl Workbench {
    /// Whether the file showing is written in as it reads: markdown or
    /// Quarto the pane holds whole, with its editor.
    pub(crate) fn doc_editable(&self) -> bool {
        !self.file_raw && self.doc_writable()
    }

    /// Whether the file showing is written in as it reads when it is shown
    /// rendered: its rendered side is the rendered editor.
    pub(crate) fn doc_writable(&self) -> bool {
        self.file_view.as_ref().is_some_and(|v| {
            matches!(&v.body, FileBody::Markdown(text) if text.len() <= DOC_MOST) && v.source.is_some() && self.file_editor.as_ref().is_some_and(|ed| ed.path == v.path && ed.saved.is_some())
        })
    }

    /// The editor the file showing is drawn and written in, made when the
    /// file is another, and kept as the file's editor has it: a change
    /// made as it is written, an undo there, the disk read anew.
    pub(crate) fn doc_canvas_now(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<live::Editor>> {
        if !self.doc_editable() {
            return None;
        }
        let path = self.file_view.as_ref()?.path.clone();
        let state = self.file_editor.as_ref()?.state.clone();
        // The file's editor read whole only when its text is not what the
        // canvas holds: this runs at every frame the pane is drawn, and a
        // copy of the file at each was a quarter of a megabyte.
        let fresh = self.doc_canvas.as_ref().is_none_or(|c| c.path != path);
        let changed = fresh || self.doc_canvas.as_ref().is_some_and(|c| !same_text(state.read(cx).text(), &c.held));
        let source = if changed { state.read(cx).value().to_string() } else { String::new() };
        let look = self.doc_look(cx);
        if fresh {
            let editor = cx.new(live::Editor::new);
            let scroll = self.file_view_scroll.clone();
            editor.update(cx, |e, cx| {
                e.set_look(look.clone(), cx);
                // A LaTeX or Typst file reads as its own markup; markdown
                // as markdown, its YAML header a header.
                let dialect = written_as(&path).map(|w| match w {
                    Written::Latex => live::Dialect::Latex,
                    Written::Typst => live::Dialect::Typst,
                });
                e.set_dialect(dialect, cx);
                e.set_header(dialect.is_none(), cx);
                e.set_text(&source, cx);
                e.scroll = scroll;
            });
            let subs = vec![cx.subscribe_in(&editor, window, |this, editor, ev: &EditorEvent, window, cx| match ev {
                EditorEvent::Changed => this.doc_changed(window, cx),
                EditorEvent::Menu(at, _) => this.doc_menu(editor, *at, cx),
                EditorEvent::OpenLink(url) => this.open_link(&SharedString::from(url.clone()), window, cx),
                _ => {}
            })];
            self.doc_canvas = Some(DocCanvas { path, editor, held: source.clone(), states: HashMap::new(), kept: KeptViews::default(), made: None, _subs: subs });
        }
        let canvas = self.doc_canvas.as_mut()?;
        if changed && canvas.held != source {
            canvas.held = source.clone();
            canvas.editor.update(cx, |e, cx| e.replace_text_quietly(&source, cx));
        }
        let editor = canvas.editor.clone();
        editor.update(cx, |e, cx| {
            if *e.look() != look {
                e.set_look(look, cx);
            }
        });
        self.doc_pieces(cx);
        Some(editor)
    }

    /// What was typed goes into the file's editor: the stretch that
    /// differs and no more, a step of its undo.
    fn doc_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(canvas) = self.doc_canvas.as_mut() else { return };
        let now = canvas.editor.read(cx).text();
        if now == canvas.held {
            return;
        }
        let Some(state) = self.file_editor.as_ref().filter(|ed| ed.path == canvas.path).map(|ed| ed.state.clone()) else { return };
        let old = std::mem::replace(&mut canvas.held, now.clone());
        let head = old.bytes().zip(now.bytes()).take_while(|(a, b)| a == b).count();
        let head = (0..=head).rev().find(|n| old.is_char_boundary(*n) && now.is_char_boundary(*n)).unwrap_or(0);
        let most = old.len().min(now.len()) - head;
        let tail = old.bytes().rev().zip(now.bytes().rev()).take(most).take_while(|(a, b)| a == b).count();
        let tail = (0..=tail).rev().find(|n| old.is_char_boundary(old.len() - n) && now.is_char_boundary(now.len() - n)).unwrap_or(0);
        let span = head..old.len() - tail;
        let with = now[head..now.len() - tail].to_string();
        state.update(cx, |s, cx| {
            if s.value().as_ref() == old.as_str() {
                let caret = span.start + with.len();
                s.replace_bytes(span, &with, caret, window, cx);
            }
        });
        cx.notify();
    }

    /// How the file is drawn: the conversation's face with its CJK face
    /// behind it, at the pane's size; the reading view's headings, tables,
    /// strong words and room between blocks.
    pub(crate) fn doc_look(&self, cx: &App) -> live::Look {
        let theme = cx.theme();
        let size = self.cfg.app.text_sizes().preview;
        let mut look = live::Look::of(theme);
        match crate::fonts::chat_family(&self.cfg.app.chat_font, cx) {
            Some(family) => {
                look.text_fallbacks = Some(crate::fonts::cjk_fallbacks(&family, cx.global::<crate::fonts::ChatFonts>()));
                look.text_family = family;
            }
            None => {
                look.text_family = theme.font_family.clone();
                look.text_fallbacks = None;
            }
        }
        look.mono_family = theme.mono_font_family.clone();
        look.inline_family = Some(crate::fonts::inline_mono_family(cx));
        look.text_px = size;
        look.line_px = (size * 1.6).round();
        // A block of code goes by the file's size, a little under it.
        look.mono_px = crate::fonts::code_px(size);
        look.colors.plate = theme.muted;
        let h = |scale: f32| live::Heading { scale, line: 1.6, above: 0. };
        look.headings = Some([h(1.7), h(1.5), h(1.25), h(1.125), h(1.), h(1.)]);
        look.cells = Some((1., 1.4));
        look.plain_head = true;
        look.strong = FontWeight::SEMIBOLD;
        // Blocks set apart as the reading view sets them: a blank line is
        // the room under a heading or a paragraph, at the window's rem.
        look.blank = Some((0.3 * 16., 0.6 * 16.));
        look
    }

    /// The lines the reading view draws, made again when the text or the
    /// look has changed.
    fn doc_pieces(&mut self, cx: &mut Context<Self>) {
        let Some(canvas) = self.doc_canvas.as_ref() else { return };
        let dark = cx.theme().mode.is_dark();
        let size = self.cfg.app.text_sizes().preview;
        let face = crate::fonts::chat_family(&self.cfg.app.chat_font, cx);
        let pad = DOC_PAD;
        let quarto = reads_as_quarto(&canvas.path);
        let look_key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (dark, size.to_bits(), &face, quarto.is_some()).hash(&mut h);
            h.finish()
        };
        let editor = canvas.editor.clone();
        let made = (editor.read(cx).version(), look_key);
        if canvas.made == Some(made) {
            return;
        }
        let source = editor.read(cx).text();
        let path = canvas.path.clone();
        let shown = match edited_body(&path, &source) {
            FileBody::Markdown(shown) => shown,
            _ => source.clone(),
        };
        let map = Map::new(&source, &shown);
        let header = header_whole(&source);
        // A LaTeX or Typst file, and a raw block of either: the runs of
        // the document's own code folded, drawn as one quiet row, as the
        // room they make, or as the picture they place.
        let folds = editor.read(cx).folds();
        let fold_pieces = self.fold_pieces(&path, folds, look_key, cx);
        if written_as(&path).is_some() {
            if let Some(canvas) = self.doc_canvas.as_mut() {
                canvas.states.retain(|k, _| fold_pieces.iter().any(|p| p.key == *k));
                canvas.made = Some(made);
            }
            editor.update(cx, |e, cx| e.set_pieces(fold_pieces, cx));
            return;
        }
        let Some(canvas) = self.doc_canvas.as_mut() else { return };
        let kept = canvas.kept.clone();
        let mut pieces = Vec::new();
        let mut used = std::collections::HashSet::new();
        // The lines before each piece, counted on from the one before it:
        // from the file's start each time, a long file with many pieces
        // took a tenth of a second a key.
        let (mut counted, mut lines_before) = (0, 0);
        for range in piece_ranges(&source, quarto.is_some()) {
            if range.start < counted {
                (counted, lines_before) = (0, 0);
            }
            lines_before += source.as_bytes()[counted..range.start].iter().filter(|b| **b == b'\n').count();
            counted = range.start;
            let first = lines_before;
            let body = source[range.clone()].trim_end_matches('\n');
            let last = first + body.matches('\n').count();
            let is_header = header.as_ref() == Some(&range);
            let words = if is_header {
                emaki_core::files::front_matter(&shown).map(|(fields, _)| emaki_core::files::front_matter_table(&fields)).unwrap_or_default()
            } else {
                map.range_to_shown(range.clone()).and_then(|r| shown.get(r)).unwrap_or(body).trim_end_matches('\n').to_string()
            };
            let key = {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (&words, is_header, look_key).hash(&mut h);
                h.finish()
            };
            used.insert(key);
            let state = canvas.states.entry(key).or_insert_with(|| cx.new(|cx| TextViewState::markdown(&words, cx))).clone();
            let (kept, face) = (kept.clone(), face.clone());
            let draw: Rc<dyn Fn(&mut Window, &mut App) -> AnyElement> = if is_header {
                Rc::new(move |_, cx| {
                    let theme = cx.theme();
                    let band = div()
                        .px(px(pad.0))
                        .py(px(12.))
                        .bg(theme.muted.opacity(if theme.mode.is_dark() { 0.5 } else { 0.7 }))
                        .border_b_1()
                        .border_color(theme.border)
                        .text_size(px(size))
                        .child(crate::transcript::md_view_fields(state.clone(), words.clone(), kept.clone(), cx));
                    div().w_full().pb(px(pad.1)).child(band).into_any_element()
                })
            } else {
                Rc::new(move |_, cx| {
                    div()
                        .w_full()
                        .text_size(px(size))
                        .line_height(relative(1.6))
                        .when_some(face.clone(), |d, face| crate::fonts::Faced::faced(d, face, cx))
                        .child(crate::transcript::md_view_piece(state.clone(), words.clone(), quarto, kept.clone(), crate::fonts::code_px(size), cx))
                        .into_any_element()
                })
            };
            let out = if is_header { (pad.0, pad.0, pad.1) } else { (0., 0., 0.) };
            pieces.push(live::Piece { first, last, key, out, draw });
        }
        used.extend(fold_pieces.iter().map(|p| p.key));
        pieces.extend(fold_pieces);
        pieces.sort_by_key(|p| p.first);
        canvas.states.retain(|k, _| used.contains(k));
        canvas.made = Some(made);
        editor.update(cx, |e, cx| e.set_pieces(pieces, cx));
    }

    /// The folds of a document's own code as pieces (`live::Fold`): a run
    /// of code one quiet row, its icon, its first words worth reading and
    /// how many lines it holds; vertical spacing the room it makes; a
    /// picture's call the picture, drawn as a markdown picture is.
    fn fold_pieces(&mut self, path: &Path, folds: Vec<live::Fold>, look_key: u64, cx: &mut Context<Self>) -> Vec<live::Piece> {
        let size = self.cfg.app.text_sizes().preview;
        let quarto = reads_as_quarto(path);
        let icon = match written_as(path) {
            Some(Written::Latex) => "icons/lang/latex.svg",
            _ => "icons/lang/typst.svg",
        };
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let Some(canvas) = self.doc_canvas.as_mut() else { return Vec::new() };
        let kept = canvas.kept.clone();
        let mut out = Vec::new();
        for fold in folds {
            let key = {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (format!("{:?}", fold.kind), fold.last - fold.first, look_key).hash(&mut h);
                h.finish()
            };
            let lines = fold.last - fold.first + 1;
            let draw: Rc<dyn Fn(&mut Window, &mut App) -> AnyElement> = match fold.kind {
                live::FoldKind::Code { summary } => Rc::new(move |_, cx| {
                    let theme = cx.theme();
                    div()
                        .w_full()
                        .py(px(3.))
                        .flex()
                        .child(
                            h_flex()
                                .max_w_full()
                                .min_w_0()
                                .gap(px(7.))
                                .px(px(9.))
                                .py(px(3.))
                                .rounded(px(7.))
                                .bg(theme.muted.opacity(0.55))
                                .text_size(px(crate::fonts::code_px(size)))
                                .text_color(theme.muted_foreground)
                                .child(svg().path(icon).size(px(12.)).flex_shrink_0().text_color(theme.muted_foreground))
                                .child(div().min_w_0().truncate().font_family(theme.mono_font_family.clone()).child(summary.clone()))
                                .child(div().flex_shrink_0().opacity(0.8).child(count(lines, "{n} line", "{n} lines"))),
                        )
                        .into_any_element()
                }),
                live::FoldKind::Space(h) => Rc::new(move |_, _| div().w_full().h(px(h)).into_any_element()),
                live::FoldKind::Picture { path: pic, width } => {
                    let at = dir.join(&pic);
                    let words = format!("![](<{}>){}", at.display(), width.map(|w| format!("{{width={w}}}")).unwrap_or_default());
                    let state = canvas.states.entry(key).or_insert_with(|| cx.new(|cx| TextViewState::markdown(&words, cx))).clone();
                    let kept = kept.clone();
                    Rc::new(move |_, cx| div().w_full().text_size(px(size)).child(crate::transcript::md_view_piece(state.clone(), words.clone(), quarto, kept.clone(), crate::fonts::code_px(size), cx)).into_any_element())
                }
            };
            out.push(live::Piece { first: fold.first, last: fold.last, key, out: (0., 0., 0.), draw });
        }
        out
    }

    /// A right click in the file: Cut and Copy for what is selected,
    /// Paste, Select All.
    fn doc_menu(&mut self, editor: &Entity<live::Editor>, at: Point<Pixels>, cx: &mut Context<Self>) {
        let (selection, focus) = {
            let s = editor.read(cx);
            (s.has_selection(), s.focus_handle(cx))
        };
        let mut items = Vec::new();
        if selection {
            items.push((crate::i18n::phrase("Cut"), MenuDo::Edit(crate::workbench::EditDo::Cut, focus.clone())));
            items.push((crate::i18n::phrase("Copy"), MenuDo::Edit(crate::workbench::EditDo::Copy, focus.clone())));
        }
        if cx.read_from_clipboard().is_some() {
            items.push((crate::i18n::phrase("Paste"), MenuDo::Edit(crate::workbench::EditDo::Paste, focus.clone())));
        }
        if !items.is_empty() {
            items.push(("", MenuDo::Rule));
        }
        items.push((crate::i18n::phrase("Select All"), MenuDo::Edit(crate::workbench::EditDo::SelectAll, focus)));
        self.open_menu(at, items, cx);
    }

    /// ⌘A with the keyboard in the pane: the keyboard to the file, all of
    /// it selected, where it stands.
    pub(crate) fn doc_select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(editor) = self.doc_canvas.as_ref().filter(|_| self.doc_editable()).map(|c| c.editor.clone()) else { return false };
        editor.update(cx, |e, cx| {
            window.focus(&e.focus_handle(cx), cx);
            e.select_whole(cx);
        });
        true
    }

    /// A press on the pane under the file's last line: the caret at the
    /// file's end, as a press under a note's last line in Onyx.
    pub(crate) fn doc_pane_pressed(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.doc_canvas.as_ref().filter(|_| self.doc_editable()).map(|c| c.editor.clone()) else { return };
        if editor.read(cx).foot().is_none_or(|foot| at.y <= foot) {
            return;
        }
        editor.update(cx, |e, cx| {
            window.focus(&e.focus_handle(cx), cx);
            e.move_to_end(cx);
        });
    }

    /// Esc in the file: the keyboard goes back to the pane, and the file
    /// reads whole. False when the keyboard is not in the file.
    pub(crate) fn doc_leave(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(editor) = self.doc_canvas.as_ref().map(|c| c.editor.clone()) else { return false };
        if !editor.read(cx).focus_handle(cx).is_focused(window) {
            return false;
        }
        window.focus(&self.file_focus, cx);
        true
    }

    /// The file and its editor's text, printed: a probe's.
    pub(crate) fn doc_dump(&self, window: &Window, cx: &App) {
        match &self.doc_canvas {
            Some(c) => {
                let s = c.editor.read(cx);
                eprintln!("emaki: doc canvas holds {:?} selection {:?} keyboard {}", s.text(), s.selection_bytes(), s.focus_handle(cx).is_focused(window));
            }
            None => eprintln!("emaki: doc canvas none"),
        }
        if let Some(ed) = &self.file_editor {
            eprintln!("emaki: doc text {:?} dirty {}", ed.state.read(cx).value().to_string(), ed.dirty(cx));
        }
    }

    /// Words typed in the file as the keyboard types them: a probe's.
    pub(crate) fn doc_probe_type(&mut self, words: &str, window: &mut Window, cx: &mut Context<Self>) {
        if words == "\0dump" {
            self.doc_dump(window, cx);
        } else if let Some(editor) = self.doc_canvas.as_ref().map(|c| c.editor.clone()) {
            editor.update(cx, |s, cx| gpui::EntityInputHandler::replace_text_in_range(s, None, words, window, cx));
        }
    }
}

/// Whether a rope holds `text`, read in its own pieces, with nothing
/// copied.
fn same_text(rope: &ropey::Rope, text: &str) -> bool {
    let mut at = 0;
    rope.len() == text.len()
        && rope.chunks().all(|c| {
            let same = text.as_bytes().get(at..at + c.len()) == Some(c.as_bytes());
            at += c.len();
            same
        })
}

/// The blocks of a markdown text the editor does not draw, which the
/// reading view draws over their lines: the YAML header; a picture, a
/// footnote, a block of HTML, anything in a fenced div (`:::`); and in a
/// Quarto document a formula, a chunk, a shortcode, a reference or a
/// citation, a span's or a heading's attributes. Each is the bytes of
/// its block, in order.
pub(crate) fn piece_ranges(source: &str, quarto: bool) -> Vec<Range<usize>> {
    use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};
    let mut out = Vec::new();
    let body = match header_whole(source) {
        Some(r) => {
            out.push(r.clone());
            r.end
        }
        None => 0,
    };
    // Fenced divs, which the parser does not know: from an opening line
    // to the closing one that matches it, outside code.
    let mut divs: Vec<Range<usize>> = Vec::new();
    let (mut depth, mut start, mut fence): (usize, usize, Option<&str>) = (0, 0, None);
    let mut at = body;
    for line in source[body..].split_inclusive('\n') {
        let t = line.trim();
        let mark = if t.starts_with("```") { Some("```") } else if t.starts_with("~~~") { Some("~~~") } else { None };
        match (fence, mark) {
            (Some(f), Some(m)) if f == m && t.trim_start_matches(|c| c == '`' || c == '~').is_empty() => fence = None,
            (None, Some(m)) => fence = Some(m),
            (None, None) if t.starts_with(":::") => {
                if t.trim_start_matches(':').trim().is_empty() {
                    if depth > 0 {
                        depth -= 1;
                        if depth == 0 {
                            divs.push(start..at + line.len());
                        }
                    }
                } else {
                    if depth == 0 {
                        start = at;
                    }
                    depth += 1;
                }
            }
            _ => {}
        }
        at += line.len();
    }
    if depth > 0 {
        divs.push(start..source.len());
    }
    let foreign = |block: &str| {
        let t = block.trim_start();
        t.starts_with('<')
            || block.contains("![")
            || block.contains("<img")
            || block.contains("[^")
            || (quarto && (block.contains('$') || block.contains("{{<") || block.contains("]{") || (t.starts_with('#') && t.lines().next().is_some_and(|l| l.trim_end().ends_with('}'))) || has_reference(block)))
    };
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_FOOTNOTES);
    let mut level = 0usize;
    let mut block: Option<(usize, bool)> = None;
    for (event, range) in Parser::new_ext(&source[body..], options).into_offset_iter() {
        let range = range.start + body..range.end + body;
        match event {
            Event::Start(tag) => {
                if level == 0 {
                    let own = match &tag {
                        Tag::HtmlBlock | Tag::FootnoteDefinition(_) => true,
                        // A chunk and a raw block are code the editor sets
                        // as written; raw HTML is drawn as the page it is.
                        Tag::CodeBlock(CodeBlockKind::Fenced(lang)) => quarto && lang.trim().replace(' ', "").eq_ignore_ascii_case("{=html}"),
                        Tag::CodeBlock(_) | Tag::Table(_) => false,
                        _ => foreign(&source[range.clone()]),
                    };
                    block = Some((range.start, own));
                }
                level += 1;
            }
            Event::End(_) => {
                level = level.saturating_sub(1);
                if level == 0 {
                    if let Some((from, true)) = block.take() {
                        out.push(from..range.end);
                    }
                }
            }
            _ => {}
        }
    }
    // A block inside a div is the div's.
    out.retain(|r| !divs.iter().any(|d| d.start < r.end && r.start < d.end));
    out.extend(divs);
    out.sort_by_key(|r| r.start);
    out
}

/// A file's YAML header with its fences, from the file's start to the
/// end of its closing line.
pub(crate) fn header_whole(source: &str) -> Option<Range<usize>> {
    let fields = text::header_range(source)?;
    let close = source[fields.end..].split_inclusive('\n').take(2).map(str::len).sum::<usize>();
    Some(0..(fields.end + close).min(source.len()))
}

/// Whether words hold a Quarto reference or citation: `@` after no
/// letter, before one.
fn has_reference(block: &str) -> bool {
    let b = block.as_bytes();
    b.iter().enumerate().any(|(i, c)| *c == b'@' && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) && b.get(i + 1).is_some_and(|n| n.is_ascii_alphabetic()))
}

#[cfg(test)]
mod tests {
    use super::piece_ranges;

    fn pieces(text: &str, quarto: bool) -> Vec<&str> {
        piece_ranges(text, quarto).into_iter().map(|r| text[r].trim_end()).collect()
    }

    #[test]
    fn what_the_editor_draws_is_no_piece() {
        assert!(pieces("# Title\n\nWords **bold** and `code`.\n\n- a\n- b\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```r\nx <- 1\n```\n", false).is_empty());
    }

    #[test]
    fn the_header_a_picture_and_a_div_are_pieces() {
        let text = "---\ntitle: A\n---\n\nWords.\n\n![a](b.png)\n\n::: {.callout-note}\nInside.\n\n```\n:::\n```\n:::\n\nAfter.\n";
        assert_eq!(pieces(text, true), vec!["---\ntitle: A\n---", "![a](b.png)", "::: {.callout-note}\nInside.\n\n```\n:::\n```\n:::"]);
    }

    #[test]
    fn quarto_has_more_than_markdown() {
        let text = "Cost $5 and $x^2$.\n\n```{r}\nx\n```\n\n```{=typst}\n#v(6pt)\n```\n\n```{=html}\n<b>x</b>\n```\n\nSee @fig-a.\n\nMail me@home.\n";
        assert!(pieces(text, false).is_empty());
        assert_eq!(pieces(text, true), vec!["Cost $5 and $x^2$.", "```{=html}\n<b>x</b>\n```", "See @fig-a."]);
    }
}
