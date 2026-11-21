//! Embeds the Nexus icon and version info into `nexus-os.exe` when building
//! for Windows. Needs `windres` (mingw-w64 or binutils); skipped quietly if it
//! is not installed so a plain `cargo check` never fails because of it.

use std::path::PathBuf;
use std::process::Command;

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
    let icon_path = icon.canonicalize().unwrap_or(icon).display().to_string().replace('\\', "/");
    // gpui embeds its own manifest only when the build host is Windows; when
    // cross-compiling, add ours (PerMonitorV2 DPI, Common Controls 6, Win10+).
    let manifest = manifest.join("../../packaging/windows/nexus.manifest.xml");
    let host_is_windows = std::env::var("HOST").is_ok_and(|h| h.contains("windows"));
    let manifest_line = if !host_is_windows && manifest.exists() {
        let m = manifest
            .canonicalize()
            .unwrap_or(manifest)
            .display()
            .to_string()
            .replace('\\', "/");
        format!("1 24 \"{m}\"\n")
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
    let obj = out.join("nexus-res.o");
    for tool in ["x86_64-w64-mingw32-windres", "windres"] {
        let ok = Command::new(tool)
            .arg(&rc)
            .arg("-O")
            .arg("coff")
            .arg("-o")
            .arg(&obj)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            println!("cargo:rustc-link-arg-bin=nexus-os={}", obj.display());
            return;
        }
    }
    println!("cargo:warning=windres not found; nexus-os.exe will have no icon");
}
