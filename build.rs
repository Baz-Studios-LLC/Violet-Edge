fn main() {
    // Embed the app icon into the Windows .exe so Explorer / the taskbar show it on the FILE itself
    // (the running window's icon is set separately at startup via winit). No-op on other platforms.
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(e) = res.compile() {
            // don't fail the build if the resource compiler is unavailable — just ship without the icon
            println!("cargo:warning=failed to embed the Windows exe icon: {e}");
        }
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
}
