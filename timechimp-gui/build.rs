// Embeds the app icon in the Windows executable. gpui loads icon resource 1 for its windows,
// so it shows in the title bar and taskbar as well as on the .exe itself.
fn main() {
    println!("cargo:rerun-if-changed=assets/rechimped.rc");
    println!("cargo:rerun-if-changed=assets/rechimped.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/rechimped.rc", embed_resource::NONE).manifest_required().unwrap();
    }
}
