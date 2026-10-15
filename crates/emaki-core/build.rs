//! libllama for the dictation's clean-up (`src/clean.rs`), on a Mac
//! alone, as dictation is.
//!
//! llama.cpp's own sources (`vendor/llama.cpp`) and the shim beside them
//! (`native/llm.cpp`) are compiled with `cc` against the ggml headers
//! transcribe-cpp-sys installs, and link against its ggml: one copy of
//! ggml in the program. `vendor/llama.cpp/UPSTREAM.md` says why the two
//! fit.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=native/llm.cpp");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../vendor/llama.cpp");
    println!("cargo:rerun-if-changed={}", root.display());
    // transcribe-cpp-sys's `links = "transcribe"` hands its install's
    // headers to the crates that depend on it.
    let ggml_include = std::env::var("DEP_TRANSCRIBE_INCLUDE_DIR").expect("transcribe-cpp-sys gives its include directory");

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .opt_level(2)
        .warnings(false)
        .include(root.join("include"))
        .include(root.join("src"))
        .include(&ggml_include)
        .file("native/llm.cpp");
    for dir in [root.join("src"), root.join("src/models")] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "cpp")).collect();
        files.sort();
        build.files(files);
    }
    build.compile("emaki_llama");
}
