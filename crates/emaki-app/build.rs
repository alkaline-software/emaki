// On Windows the executable's icon is a resource compiled into it; the
// `.ico` is the one scripts/make-icon.sh draws. Elsewhere there is nothing
// to do: macOS reads the bundle's .icns, Linux the desktop entry's PNG.
fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/icon.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon resource not embedded: {e}");
        }
    }
    println!("cargo:rerun-if-changed=assets/icon/icon.ico");
}
