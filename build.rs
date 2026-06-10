// On macOS with a non-Apple (e.g. Nix) clang, the linker doesn't search the
// Xcode SDK for system libs like `iconv`. Add the SDK lib dir explicitly so
// `-liconv` (pulled in transitively) resolves.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos")
        && let Ok(out) = std::process::Command::new("xcrun")
            .args(["--show-sdk-path"])
            .output()
            && out.status.success() {
                let sdk = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !sdk.is_empty() {
                    println!("cargo:rustc-link-search=native={sdk}/usr/lib");
                }
            }
}
