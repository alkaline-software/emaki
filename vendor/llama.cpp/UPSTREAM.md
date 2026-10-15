# llama.cpp, the library alone

From ggml-org/llama.cpp at commit
`e117148a41d8e9bedb72e4c6c3f003ab0fe7f857` (2026-10-05): `include/`,
`src/` and `LICENSE` (MIT), and nothing else. It runs the dictation's
clean-up models (`crates/emaki-core/src/clean.rs`).

**It is built against the ggml that transcribe.cpp brings, never one of
its own.** Two copies of ggml in one program clash, which is why
whisper-rs went. transcribe-cpp 0.4 synced its ggml from this very
commit (`ggml/scripts/sync-llama.last` in its crate) and adds only a
backend-registration filter and two thread fixes, so libllama from here
links against it as it is. `crates/emaki-core/build.rs` compiles these
sources with the `cc` crate, against the headers transcribe-cpp-sys
installs (`DEP_TRANSCRIBE_INCLUDE_DIR`), in place of llama.cpp's CMake.

Changes made here:

- `src/CMakeLists.txt` and `src/llama-version.h.in` are left out;
  `src/llama-version.h` is written by hand.

## Updating

When transcribe-cpp is updated, read its `ggml/scripts/sync-llama.last`,
copy `include/`, `src/` and `LICENSE` from that llama.cpp commit over
these, write `src/llama-version.h` again, and build. A libllama from any
other commit is built against a ggml it was not written for.
