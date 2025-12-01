use std::{env, fs, path::Path};

fn main() {
    let project = env!("CARGO_PKG_NAME");
    let issue_tracker = read_issue_tracker();
    #[cfg(not(feature = "setup"))]
    let authors: Vec<_> = env!("CARGO_PKG_AUTHORS").split(':').collect();
    #[cfg(not(feature = "setup"))]
    let repository = env!("CARGO_PKG_REPOSITORY");
    #[cfg(not(feature = "setup"))]
    let version = env!("CARGO_PKG_VERSION");
    #[cfg(not(feature = "setup"))]
    let summary = env::var("CARGO_PKG_DESCRIPTION").unwrap_or_else(|_| "Anime/Japanese Radio".to_string());
    #[cfg(not(feature = "setup"))]
    let homepage = env::var("CARGO_PKG_HOMEPAGE").unwrap_or_else(|_| "https://listen.moe/".to_string());
    #[cfg(not(feature = "setup"))]
    let license = env::var("CARGO_PKG_LICENSE").unwrap_or_else(|_| "MIT".to_string());

    let app_id = if cfg!(debug_assertions) {
        format!("dev.noobping.{project}.develop")
    } else {
        format!("dev.noobping.{project}")
    };
    let resource_id = format!("/dev/noobping/{project}");

    // Expose APP_ID and RESOURCE_ID to your main crate:
    println!("cargo:rustc-env=APP_ID={app_id}");
    println!("cargo:rustc-env=RESOURCE_ID={resource_id}");
    println!("cargo:rustc-env=ISSUE_TRACKER={issue_tracker}");

    // Make Cargo rerun if these change
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=data");

    // Ensure data/ exists
    let data_dir = Path::new("data");
    fs::create_dir_all(&data_dir).unwrap();

    // Collect all .svg icon files under data/
    let mut icons: Vec<String> = Vec::new();
    collect_svg_icons(&data_dir, &data_dir, &mut icons);
    icons.sort();

    // Generate data/resources.xml
    let mut xml = String::from("<gresources>\n");
    xml.push_str(&format!("\t<gresource prefix=\"{resource_id}\">\n"));
    for f in &icons {
        xml.push_str(&format!("\t\t<file>{}</file>\n", f));
    }
    xml.push_str("\t</gresource>\n</gresources>\n");
    fs::write(data_dir.join(format!("{app_id}.resources.xml")), xml).unwrap();

    // Compile GResources into $OUT_DIR/compiled.gresource
    glib_build_tools::compile_resources(&["data"], &format!("data/{app_id}.resources.xml"), "compiled.gresource");

    // Generate .desktop file for non-setup builds
    #[cfg(not(feature = "setup"))]
    {
        desktop_file(&data_dir, &project, &version, &summary, &app_id);
        metainfo_file(&data_dir, &app_id, &authors.first().expect("unknown author"), &repository, &project, &summary, &homepage, &license, &version, &issue_tracker);
    }
}

/// Recursively collect all `.svg` files under `dir`,
/// and push their path *relative to `data_dir`* into `icons`.
fn collect_svg_icons(dir: &Path, data_dir: &Path, icons: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.is_dir() {
            collect_svg_icons(&path, data_dir, icons);
        } else if path.extension().and_then(|e| e.to_str()) == Some("svg") {
            let rel = path.strip_prefix(data_dir).unwrap();
            icons.push(rel.to_string_lossy().into_owned());
        }
    }
}

#[cfg(not(feature = "setup"))]
fn desktop_file(data_dir: &Path, project: &str, version: &str, comment: &str, app_id: &str) {
    let contents = format!(
        "[Desktop Entry]
Type=Application
Version={version}
Name={project}
Comment={comment}
Exec={project} %u
Icon={app_id}
Terminal=false
Categories=AudioVideo;Player;
"
    );
    fs::write(data_dir.join(format!("{app_id}.desktop")), contents)
        .expect("Can not build desktop file");
}

#[cfg(not(feature = "setup"))]
fn metainfo_file(
    data_dir: &Path,
    app_id: &str,
    developer: &str,
    repository: &str,
    project: &str,
    summary: &str,
    homepage: &str,
    license: &str,
    version: &str,
    issue_tracker: &str,
) {
    let contents = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>{app_id}</id>
  <name>LISTEN.moe</name>
  <summary>{summary}</summary>

  <developer_name>{developer}</developer_name>

  <metadata_license>CC0-1.0</metadata_license>
  <project_license>{license}</project_license>

  <url type="homepage">{homepage}</url>
  <url type="bugtracker">{issue_tracker}</url>
  <url type="vcs-browser">{repository}</url>

  <launchable type="desktop-id">{app_id}.desktop</launchable>

  <description>
    <p>{summary}</p>
  </description>

  <provides>
    <binary>{project}</binary>
  </provides>

  <releases>
    <release version="{version}" />
  </releases>
</component>
"#
    );

    fs::write(data_dir.join(format!("{app_id}.metainfo.xml")), contents)
        .expect("Can not write metainfo file");
}

fn read_issue_tracker() -> String {
    let cargo_toml = fs::read_to_string("Cargo.toml").expect("Cargo.toml missing");
    let value: toml::Value = toml::from_str(&cargo_toml).expect("invalid Cargo.toml");

    value
        .get("package")
        .and_then(|p| p.get("metadata"))
        .and_then(|m| m.get("issue-tracker"))
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string()
}
