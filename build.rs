fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Icon of the exe, GPUI also loads it from resource 1 for the window
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/icons/paproxy.ico");
        winresource::WindowsResource::new()
            .set_icon("assets/icons/paproxy.ico")
            .compile()
            .expect("Failed to embed Windows resources");
    }
}
