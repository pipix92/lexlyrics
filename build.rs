use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=res/win/icon.ico");

    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS")?.as_str() == "windows" {
        windows_exe_info::icon::icon_ico("res/win/icon.ico");
        windows_exe_info::versioninfo::VersionInfo::from_cargo_env_ex(
            Some("Desktop Karaoke Lyrics with Live AI Translation"),
            Some("LexLyrics"),
            None,
            None,
        )
        .link()?;
    }

    // When cross-compiling to Windows from Linux (MinGW), embed `icon0` so both
    // Explorer and `tray_item::IconSource::Resource("icon0")` find the icon.
    #[cfg(not(windows))]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let out_dir = std::env::var("OUT_DIR")?;
        let rc_path = format!("{out_dir}/lexlyrics.rc");
        let obj_path = format!("{out_dir}/lexlyrics_res.o");
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
        std::fs::write(
            &rc_path,
            format!("icon0 ICON \"{manifest_dir}/res/win/icon.ico\"\n"),
        )?;
        let windres = std::env::var("WINDRES")
            .unwrap_or_else(|_| "x86_64-w64-mingw32-windres".to_string());
        let status = std::process::Command::new(windres)
            .args([&rc_path, "-O", "coff", "-o", &obj_path])
            .status()?;
        if status.success() {
            println!("cargo:rustc-link-arg-bins={obj_path}");
        }
    }

    Ok(())
}
