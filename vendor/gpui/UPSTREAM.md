# Vendored gpui

Source: https://github.com/zed-industries/zed
Revision: ce48461eaadd16c65c31f835511ab96bd3b6e746 (the revision Cargo.lock pins for every zed crate)
Crate: crates/gpui, copied as it is from that revision without its examples, tests and docs (the `[[example]]` sections are taken out of its manifest), with the upstream LICENSE-APACHE beside it. The workspace Cargo.toml here is zed's `[workspace.package]`, its whole dependency table with each sibling crate's `path` replaced by the zed git source, and its lints, so the crate builds unchanged against the same siblings Cargo.lock already names.

Emaki's root Cargo.toml takes this copy over the git source with a `[patch]` section. The patch applies to every crate that takes gpui from that source, the vendored gpui-component included, so the build still has one gpui. The pin stays in Cargo.lock: moving it (`cargo update -p gpui --precise <rev>`) moves the siblings, and this copy has to be made again from the new revision, with the change below carried over, or dropped when the revision no longer needs it.

## Changes made here

Each is marked `(Emaki addition.)` in the source, so a diff against the upstream revision finds them.

1. `crates/gpui/src/text_system/line_wrapper.rs`: where a line may break is one rule for both wrappers, `breaks_before`, with UAX #14's main prohibitions: a mark that closes or ends (`?`, `,`, `)`, `。`, `」`, a small kana, `ー`: `no_line_start`) never begins a line, nothing follows an opening mark (`(`, `“`, `「`: `no_line_end`) onto the next line without it, and a word may begin a line after any mark a line may end on as well as after a space (`foo?b=2`, `a/b`). Upstream treated most closing punctuation as part of the word before it but left `?` and `/` as break opportunities so URLs could wrap, and every CJK character, punctuation included, as one, so "?" and "。" began lines whenever the word before them filled one.

## When this copy can go

Upstream's wrapper on `main` was rewritten on UAX #14 boundaries (`unicode_segmentation`, October 2026). When the pin moves to a revision with that wrapper, delete `vendor/gpui`, the `[patch."https://github.com/zed-industries/zed"]` section of the root Cargo.toml, and the note in docs/platform.md.
