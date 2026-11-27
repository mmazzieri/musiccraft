// The icon and version details of musiccraft.exe, so that File Explorer, shortcuts and the
// taskbar show the program's own icon, the portable build included.
//
// Nothing happens for other targets. Without a resource compiler the build goes on with a
// warning, unless MUSICCRAFT_REQUIRE_WINRES is set, as the release packaging does.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app-icon/musiccraft.ico");
    println!("cargo:rerun-if-env-changed=MUSICCRAFT_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/app-icon/musiccraft.ico")
        .set("ProductName", "MusicCraft")
        .set("FileDescription", "MusicCraft")
        .set("LegalCopyright", "MIT OR Apache-2.0")
        .set("OriginalFilename", "musiccraft.exe")
        .set("InternalName", "musiccraft");
    if let Err(e) = res.compile() {
        if std::env::var_os("MUSICCRAFT_REQUIRE_WINRES").is_some() {
            eprintln!("the Windows resources of musiccraft.exe could not be built: {e}");
            std::process::exit(1);
        }
        println!("cargo:warning=musiccraft.exe is built without its icon: {e}");
    }
}
