//! First-run desktop integration for the Linux AppImage.
//!
//! A bare AppImage carries no desktop entry, so panels and app menus cannot
//! find its icon: the window icon set in code (jolt#208) is not enough for
//! Cinnamon, which matches windows to a `.desktop` file by `StartupWMClass`.
//! The AppImage runtime exposes the mounted bundle as `$APPDIR` and the image
//! path as `$APPIMAGE`, which is everything needed to install the bundled entry
//! and icon into the user's XDG data directory. Nothing here runs for a `.deb`
//! install, which already ships both.

use std::path::{Path, PathBuf};

pub const IDENTIFIER: &str = "net.jolt.console";

/// The window's app id on Wayland and its WM_CLASS on X11. Tauri does not set
/// a GTK application id, so both fall back to the binary name. Panels match a
/// window to the desktop entry with this file name, so the entry must carry
/// it; `StartupWMClass` alone is only honoured by some X11 desktops.
pub const APP_ID: &str = "jolt-console";

pub struct AppImageContext {
    pub appimage: PathBuf,
    pub appdir: PathBuf,
}

/// Present only when running from an AppImage; both variables are set by the
/// AppImage runtime before the payload starts.
pub fn appimage_context() -> Option<AppImageContext> {
    let appimage = std::env::var_os("APPIMAGE")?;
    let appdir = std::env::var_os("APPDIR")?;
    Some(AppImageContext {
        appimage: PathBuf::from(appimage),
        appdir: PathBuf::from(appdir),
    })
}

pub fn data_home() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
}

pub struct IntegrationPaths {
    pub desktop_entry: PathBuf,
    pub icon: PathBuf,
    // Written when the user says "not now", so the offer is made once.
    pub declined_marker: PathBuf,
}

pub fn integration_paths(data_home: &Path) -> IntegrationPaths {
    IntegrationPaths {
        desktop_entry: data_home
            .join("applications")
            .join(format!("{APP_ID}.desktop")),
        icon: data_home
            .join("icons/hicolor/512x512/apps")
            .join(format!("{IDENTIFIER}.png")),
        declined_marker: data_home.join("jolt/console-menu-entry-declined"),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum IntegrationState {
    Installed,
    Declined,
    Offer,
}

pub fn integration_state(paths: &IntegrationPaths) -> IntegrationState {
    if paths.desktop_entry.exists() {
        IntegrationState::Installed
    } else if paths.declined_marker.exists() {
        IntegrationState::Declined
    } else {
        IntegrationState::Offer
    }
}

pub fn bundled_desktop_entry(appdir: &Path) -> Option<PathBuf> {
    first_with_extension(&appdir.join("usr/share/applications"), "desktop")
}

pub fn bundled_icon(appdir: &Path) -> Option<PathBuf> {
    let apps = appdir.join("usr/share/icons/hicolor/512x512/apps");
    first_with_extension(&apps, "png")
}

fn first_with_extension(dir: &Path, extension: &str) -> Option<PathBuf> {
    let mut matches: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == extension))
        .collect();
    matches.sort();
    matches.into_iter().next()
}

/// Rewrites the bundled entry so it launches this AppImage and uses the icon
/// installed alongside it. `StartupWMClass` is kept verbatim: it is how the
/// panel ties the running window to this entry.
pub fn render_desktop_entry(bundled: &str, appimage: &Path) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut saw_categories = false;
    for line in bundled.lines() {
        let rendered = if line.starts_with("Exec=") {
            format!("Exec={}", quote_exec_argument(appimage))
        } else if line.starts_with("TryExec=") {
            continue;
        } else if line.starts_with("Icon=") {
            format!("Icon={IDENTIFIER}")
        } else if let Some(value) = line.strip_prefix("Categories=") {
            saw_categories = true;
            if value.trim().is_empty() {
                "Categories=Network;".to_string()
            } else {
                line.to_string()
            }
        } else {
            line.to_string()
        };
        lines.push(rendered);
    }
    if !saw_categories {
        lines.push("Categories=Network;".to_string());
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

// Desktop entry Exec values are quoted with double quotes, inside which only
// these four characters need a backslash.
fn quote_exec_argument(path: &Path) -> String {
    let mut quoted = String::from("\"");
    for ch in path.to_string_lossy().chars() {
        if matches!(ch, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(ch);
    }
    quoted.push('"');
    quoted
}

pub fn install(context: &AppImageContext, paths: &IntegrationPaths) -> Result<(), String> {
    let entry_path = bundled_desktop_entry(&context.appdir)
        .ok_or_else(|| "the AppImage bundles no desktop entry".to_string())?;
    let icon_path =
        bundled_icon(&context.appdir).ok_or_else(|| "the AppImage bundles no icon".to_string())?;
    let bundled = std::fs::read_to_string(&entry_path)
        .map_err(|error| format!("failed to read {}: {error}", entry_path.display()))?;

    write_file(
        &paths.icon,
        &std::fs::read(&icon_path).map_err(|error| error.to_string())?,
    )?;
    write_file(
        &paths.desktop_entry,
        render_desktop_entry(&bundled, &context.appimage).as_bytes(),
    )?;
    refresh_desktop_database(paths);
    Ok(())
}

pub fn decline(paths: &IntegrationPaths) -> Result<(), String> {
    write_file(&paths.declined_marker, b"")
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    std::fs::write(path, bytes)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

// Best effort: menus pick the entry up on their own eventually, this just
// makes it immediate on desktops that keep a cache.
fn refresh_desktop_database(paths: &IntegrationPaths) {
    if let Some(applications) = paths.desktop_entry.parent() {
        let _ = std::process::Command::new("update-desktop-database")
            .arg(applications)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLED: &str = "[Desktop Entry]\nCategories=\nExec=jolt-console\nStartupWMClass=jolt-console\nIcon=jolt-console\nName=Jolt Console\nTerminal=false\nType=Application\n";

    #[test]
    fn rendered_entry_launches_the_appimage_with_the_installed_icon() {
        let rendered =
            render_desktop_entry(BUNDLED, Path::new("/home/a b/Apps/jolt-console.AppImage"));

        assert!(rendered.contains("Exec=\"/home/a b/Apps/jolt-console.AppImage\"\n"));
        assert!(rendered.contains("Icon=net.jolt.console\n"));
        assert!(rendered.contains("StartupWMClass=jolt-console\n"));
        assert!(rendered.contains("Categories=Network;\n"));
        assert!(!rendered.contains("Exec=jolt-console"));
    }

    #[test]
    fn exec_quoting_escapes_the_reserved_characters() {
        assert_eq!(
            quote_exec_argument(Path::new("/tmp/we\"ird$`\\dir/app")),
            "\"/tmp/we\\\"ird\\$\\`\\\\dir/app\""
        );
    }

    #[test]
    fn integration_paths_follow_the_xdg_layout() {
        let paths = integration_paths(Path::new("/home/x/.local/share"));

        assert_eq!(
            paths.desktop_entry,
            PathBuf::from("/home/x/.local/share/applications/jolt-console.desktop")
        );
        assert_eq!(
            paths.icon,
            PathBuf::from("/home/x/.local/share/icons/hicolor/512x512/apps/net.jolt.console.png")
        );
    }

    #[test]
    fn install_writes_entry_and_icon_from_the_mounted_bundle() {
        let appdir = tempfile::tempdir().unwrap();
        let data_home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(appdir.path().join("usr/share/applications")).unwrap();
        std::fs::create_dir_all(appdir.path().join("usr/share/icons/hicolor/512x512/apps"))
            .unwrap();
        std::fs::write(
            appdir
                .path()
                .join("usr/share/applications/Jolt Console.desktop"),
            BUNDLED,
        )
        .unwrap();
        std::fs::write(
            appdir
                .path()
                .join("usr/share/icons/hicolor/512x512/apps/jolt-console.png"),
            b"png-bytes",
        )
        .unwrap();
        let context = AppImageContext {
            appimage: PathBuf::from("/opt/jolt-console.AppImage"),
            appdir: appdir.path().to_path_buf(),
        };
        let paths = integration_paths(data_home.path());
        assert_eq!(integration_state(&paths), IntegrationState::Offer);

        install(&context, &paths).unwrap();

        assert_eq!(integration_state(&paths), IntegrationState::Installed);
        assert_eq!(std::fs::read(&paths.icon).unwrap(), b"png-bytes");
        let entry = std::fs::read_to_string(&paths.desktop_entry).unwrap();
        assert!(entry.contains("Exec=\"/opt/jolt-console.AppImage\"\n"));
    }

    #[test]
    fn declining_is_remembered() {
        let data_home = tempfile::tempdir().unwrap();
        let paths = integration_paths(data_home.path());

        decline(&paths).unwrap();

        assert_eq!(integration_state(&paths), IntegrationState::Declined);
    }
}
