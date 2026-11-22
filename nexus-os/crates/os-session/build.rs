//! Embeds the Nexus icon and version info into `nexus-os.exe` when building
//! for Windows. Needs `windres` (mingw-w64 or binutils); skipped quietly if it
//! is not installed so a plain `cargo check` never fails because of it.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../packaging/windows/nexus.ico");
    println!("cargo:rerun-if-changed=../../packaging/windows/nexus.manifest.xml");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = manifest.join("../../packaging/windows/nexus.ico");
    if !icon.exists() {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".into());
    let nums: Vec<u32> = version
        .split(['.', '-'])
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    let (a, b, c) = (
        nums.first().copied().unwrap_or(0),
        nums.get(1).copied().unwrap_or(0),
        nums.get(2).copied().unwrap_or(0),
    );
    let rc = out.join("nexus.rc");
    let icon_path = rc_path(icon);
    // gpui's own manifest is embedded from a *library* build script. With the
    // GNU toolchain that resource object is never pulled into the final exe
    // (nothing references it), so the exe had no manifest at all: no Common
    // Controls 6, so TaskDialogIndirect failed to resolve at load time. Embed
    // the full Nexus manifest here for GNU targets. MSVC links gpui's .res and
    // two manifests would collide there, so it is skipped for MSVC.
    let manifest_file = manifest.join("../../packaging/windows/nexus.manifest.xml");
    let gnu = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu");
    let manifest_line = if gnu && manifest_file.exists() {
        format!("1 24 \"{}\"\n", rc_path(manifest_file))
    } else {
        String::new()
    };
    let body = format!(
        r#"{manifest_line}1 ICON "{icon_path}"
1 VERSIONINFO
FILEVERSION {a},{b},{c},0
PRODUCTVERSION {a},{b},{c},0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "Nexus-OS"
      VALUE "FileDescription", "Nexus"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "nexus-os"
      VALUE "OriginalFilename", "nexus-os.exe"
      VALUE "ProductName", "Nexus"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    if std::fs::write(&rc, body).is_err() {
        return;
    }
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}

fn rc_path(path: PathBuf) -> String {
    let path = path.canonicalize().unwrap_or(path);
    let text = path.display().to_string();
    let text = text
        .strip_prefix(r"\\?\")
        .or_else(|| text.strip_prefix("//?/"))
        .unwrap_or(&text);
    text.replace('\\', "/")
}
