use adw::gio::{self, ResourceLookupFlags};
use std::io::{Error, ErrorKind};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::{env, fs};

use crate::config::{APP_ID, RESOURCE_ID};

pub fn can_install_locally() -> bool {
    let Some(bin) = dirs::executable_dir() else {
        return false;
    };
    let Some(data) = dirs::data_dir() else {
        return false;
    };
    let apps = data.join("applications");

    let bin_parent_is_writable = bin.parent().map(is_writable).unwrap_or(false);
    let apps_parent_is_writable = apps.parent().map(is_writable).unwrap_or(false);
    // If they exist, they must be writable; if not, the parent must be writable.
    (bin.exists() && bin.is_dir() && is_writable(&bin) || bin_parent_is_writable)
        && (apps.exists() && apps.is_dir() && is_writable(&apps) || apps_parent_is_writable)
}

pub fn is_installed_locally() -> bool {
    let Some(bin) = dirs::executable_dir() else {
        return false;
    };
    let Some(data) = dirs::data_dir() else {
        return false;
    };
    let bin = bin.join(env!("CARGO_PKG_NAME"));
    let desktop = data
        .join("applications")
        .join(format!("{}.desktop", APP_ID));
    bin.exists() && bin.is_file() && desktop.exists() && desktop.is_file()
}

pub fn install_locally() -> std::io::Result<()> {
    let project = env!("CARGO_PKG_NAME");
    let exe_path = std::env::current_exe()?;
    let Some(bin) = dirs::executable_dir() else {
        return Err(Error::new(
            ErrorKind::NotFound,
            "No executable directory found",
        ));
    };
    let Some(data) = dirs::data_dir() else {
        return Err(Error::new(ErrorKind::NotFound, "No data directory found"));
    };
    let apps = data.join("applications");
    let icons = data
        .join("icons")
        .join("hicolor")
        .join("scalable")
        .join("apps");
    let dest = bin.join(project);

    std::fs::create_dir_all(&bin)?;
    std::fs::create_dir_all(&apps)?;
    std::fs::create_dir_all(&icons)?;
    std::fs::copy(&exe_path, &dest)?;

    let mut perms = std::fs::metadata(&dest)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&dest, perms)?;

    write_desktop_file(&apps, &dest)?;
    extract_icon(&icons)?;

    Ok(())
}

pub fn uninstall_locally() -> std::io::Result<()> {
    let Some(bin) = dirs::executable_dir() else {
        return Err(Error::new(
            ErrorKind::NotFound,
            "No executable directory found",
        ));
    };
    let Some(data) = dirs::data_dir() else {
        return Err(Error::new(ErrorKind::NotFound, "No data directory found"));
    };
    let bin = bin.join(env!("CARGO_PKG_NAME"));
    let icon = data
        .join("icons")
        .join("hicolor")
        .join("scalable")
        .join("apps");
    let desktop = data
        .join("applications")
        .join(format!("{}.desktop", APP_ID));
    if bin.exists() {
        fs::remove_file(bin)?;
    }
    if desktop.exists() {
        fs::remove_file(desktop)?;
    }
    if icon.exists() {
        fs::remove_file(icon)?;
    }
    Ok(())
}

fn is_writable(dir: &Path) -> bool {
    // Try to open a temp file for writing
    let test_path = dir.join(".perm_test");
    match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&test_path)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(test_path);
            true
        }
        Err(_) => false,
    }
}

fn write_desktop_file(apps_path: &Path, bin_path: &Path) -> std::io::Result<()> {
    let project = env!("CARGO_PKG_NAME");
    let version = env!("CARGO_PKG_VERSION");
    let comment = option_env!("CARGO_PKG_DESCRIPTION").unwrap_or("Password manager");
    let exec = bin_path.display(); // absolute path to the installed binary
    let contents = format!(
        "[Desktop Entry]
Type=Application
Version={version}
Name={project}
Comment={comment}
Exec={exec} %u
Icon={APP_ID}
Terminal=false
Categories=AudioVideo;Player;
",
    );

    let file = apps_path.join(format!("{}.desktop", APP_ID));
    fs::write(&file, contents)?;

    // Make sure it's readable by the user
    let mut perms = fs::metadata(&file)?.permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&file, perms)?;

    Ok(())
}

fn extract_icon(apps_dir: &Path) -> std::io::Result<()> {
    let resource_path = format!("{}/scalable/apps/{}.svg", RESOURCE_ID, APP_ID);
    println!("Looking up resource: {resource_path}");
    let bytes = gio::resources_lookup_data(&resource_path, ResourceLookupFlags::NONE)
        .map_err(|e| Error::new(ErrorKind::NotFound, format!("Resource not found: {e}")))?;
    let out_path = apps_dir.join(format!("{}.svg", APP_ID));
    std::fs::write(&out_path, bytes.as_ref())?;
    Ok(())
}
