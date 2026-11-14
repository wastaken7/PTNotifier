fn main() {
    println!("cargo:rerun-if-changed=src/web/assets/icon.ico");

    #[cfg(windows)]
    {
        if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("src/web/assets/icon.ico");
            if let Err(e) = res.compile() {
                println!("cargo:warning=Failed to embed Windows icon resource: {e}");
            }
        }
    }
}
