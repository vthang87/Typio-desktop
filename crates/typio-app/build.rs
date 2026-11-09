//! On Windows, puts the Typio icon and the app manifest (modern controls, per-monitor DPI
//! awareness) into Typio.exe. Needs the Windows resource compiler, so only when building on
//! Windows; a cross-check from macOS skips it.

fn main() {
    println!("cargo:rerun-if-changed=windows/typio.rc");
    println!("cargo:rerun-if-changed=windows/typio.manifest");
    println!("cargo:rerun-if-changed=../../assets/AppIcon.ico");
    let target_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if target_windows && cfg!(windows) {
        embed_resource::compile("windows/typio.rc", embed_resource::NONE)
            .manifest_required()
            .expect("could not compile the Windows resources");
    }
}
