use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::bail;
use serde::Deserialize;

const REPO: &str = "Roger-luo/ceo";
const TAG_PREFIX: &str = "ceo-v";
const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

// ── GitHub API types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

// ── Platform detection ───────────────────────────────────────────────

fn target_triple() -> String {
    let arch = std::env::consts::ARCH; // aarch64 | x86_64
    let os_part = match std::env::consts::OS {
        "macos" => "apple-darwin",
        "linux" => "unknown-linux-gnu",
        "windows" => "pc-windows-msvc",
        other => other,
    };
    format!("{arch}-{os_part}")
}

fn match_asset(asset_names: &[String]) -> Option<String> {
    let triple = target_triple();
    let pattern = format!("ceo-{triple}");

    // Exact target triple match
    if let Some(name) = asset_names.iter().find(|n| n.starts_with(&pattern)) {
        return Some(name.clone());
    }

    // Fallback: OS + arch aliases
    let os_aliases: &[&str] = match std::env::consts::OS {
        "macos" => &["darwin", "macos", "apple"],
        "linux" => &["linux"],
        _ => &[],
    };
    let arch_aliases: &[&str] = match std::env::consts::ARCH {
        "x86_64" => &["x86_64", "amd64", "x64"],
        "aarch64" => &["aarch64", "arm64"],
        _ => &[],
    };

    for name in asset_names {
        let lower = name.to_lowercase();
        if !lower.starts_with("ceo") || !lower.ends_with(".tar.gz") {
            continue;
        }
        let has_os = os_aliases.iter().any(|a| lower.contains(a));
        let has_arch = arch_aliases.iter().any(|a| lower.contains(a));
        if has_os && has_arch {
            return Some(name.clone());
        }
    }
    None
}

// ── Package manager detection ────────────────────────────────────────

fn detect_package_manager() -> Option<&'static str> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let exe_str = exe.to_str()?;

    if let Some(cargo_home) = std::env::var_os("CARGO_HOME") {
        if exe.starts_with(cargo_home) {
            return Some("cargo");
        }
    } else if let Some(home) = dirs::home_dir() {
        if exe.starts_with(home.join(".cargo/bin")) {
            return Some("cargo");
        }
    }

    if exe_str.contains("/Cellar/") || exe_str.starts_with("/opt/homebrew/bin/") {
        return Some("brew");
    }

    None
}

fn update_command_hint() -> &'static str {
    match detect_package_manager() {
        Some("cargo") => "cargo install --git https://github.com/Roger-luo/ceo --force",
        Some("brew") => "brew upgrade ceo",
        _ => "ceo self update",
    }
}

// ── GitHub API helpers ───────────────────────────────────────────────

fn github_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::new()
}

fn fetch_latest_release() -> anyhow::Result<GitHubRelease> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=10");
    let resp = github_client()
        .get(&url)
        .header("User-Agent", "ceo-cli")
        .header("Accept", "application/vnd.github+json")
        .send()?;
    if !resp.status().is_success() {
        bail!("GitHub API returned {}", resp.status());
    }
    let releases: Vec<GitHubRelease> = resp.json()?;
    releases
        .into_iter()
        .find(|r| r.tag_name.starts_with(TAG_PREFIX) && !r.assets.is_empty())
        .ok_or_else(|| anyhow::anyhow!("No release found with tag prefix '{TAG_PREFIX}'"))
}

fn fetch_release_by_version(version: &str) -> anyhow::Result<GitHubRelease> {
    let ver = version.strip_prefix('v').unwrap_or(version);
    let tag = format!("{TAG_PREFIX}{ver}");
    let url = format!("https://api.github.com/repos/{REPO}/releases/tags/{tag}");
    let resp = github_client()
        .get(&url)
        .header("User-Agent", "ceo-cli")
        .header("Accept", "application/vnd.github+json")
        .send()?;
    if !resp.status().is_success() {
        bail!("GitHub API returned {} for tag '{tag}'", resp.status());
    }
    Ok(resp.json()?)
}

fn parse_version_from_tag(tag: &str) -> &str {
    if let Some(pos) = tag.rfind("-v") {
        &tag[pos + 2..]
    } else {
        tag.strip_prefix('v').unwrap_or(tag)
    }
}

fn download_file(url: &str, dest: &Path) -> anyhow::Result<()> {
    let resp = github_client()
        .get(url)
        .header("User-Agent", "ceo-cli")
        .send()?;
    if !resp.status().is_success() {
        bail!("Download returned {}", resp.status());
    }
    let bytes = resp.bytes()?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(dest, &bytes)?;
    Ok(())
}

fn extract_tar_gz(archive_path: &Path, dest_dir: &Path) -> anyhow::Result<()> {
    let file = std::fs::File::open(archive_path)?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    std::fs::create_dir_all(dest_dir)?;
    archive.unpack(dest_dir)?;
    Ok(())
}

fn find_binary_in_dir(dir: &Path) -> anyhow::Result<PathBuf> {
    let direct = dir.join("ceo");
    if direct.is_file() {
        return Ok(direct);
    }
    // Search one level of subdirectories
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.path().is_dir() {
            let nested = entry.path().join("ceo");
            if nested.is_file() {
                return Ok(nested);
            }
        }
    }
    bail!("Could not find 'ceo' binary in extracted archive")
}

fn replace_exe(new_binary: &Path) -> anyhow::Result<PathBuf> {
    let current_exe = std::env::current_exe()?.canonicalize()?;
    let backup = current_exe.with_extension("old");

    if let Err(e) = std::fs::rename(&current_exe, &backup) {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            bail!("Permission denied. Try: sudo ceo self update");
        }
        bail!("Failed to back up current executable: {e}");
    }

    if let Err(e) = std::fs::copy(new_binary, &current_exe) {
        let _ = std::fs::rename(&backup, &current_exe);
        bail!("Failed to install new binary: {e}");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&current_exe, std::fs::Permissions::from_mode(0o755))?;
    }

    let _ = std::fs::remove_file(&backup);
    Ok(current_exe)
}

// ── Version comparison ───────────────────────────────────────────────

fn parse_version_tuple(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

fn is_newer_version(current: &str, latest: &str) -> bool {
    match (parse_version_tuple(current), parse_version_tuple(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => current != latest,
    }
}

// ── Public commands ──────────────────────────────────────────────────

pub fn info() -> anyhow::Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    let target = env!("TARGET");
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("unknown"));

    println!("ceo {version}");
    println!("target: {target}");
    println!("exe: {}", exe.display());
    Ok(())
}

pub fn check() -> anyhow::Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let release = fetch_latest_release()?;
    let latest = parse_version_from_tag(&release.tag_name);

    println!("installed: {current}");
    println!("latest:    {latest}");

    if !is_newer_version(current, latest) {
        println!("\nAlready up to date.");
    } else {
        println!("\nUpdate available: {current} -> {latest}");
        println!("Run `{}` to install it.", update_command_hint());
    }
    Ok(())
}

pub fn update(version: Option<&str>) -> anyhow::Result<()> {
    if let Some(manager) = detect_package_manager() {
        let hint = match manager {
            "cargo" => "cargo install --git https://github.com/Roger-luo/ceo --force",
            "brew" => "brew upgrade ceo",
            _ => "your package manager",
        };
        bail!(
            "ceo was installed via {manager}, so `ceo self update` cannot safely replace it.\n\
             Update with: {hint}"
        );
    }

    let current = env!("CARGO_PKG_VERSION");
    let release = match version {
        Some(v) => fetch_release_by_version(v)?,
        None => fetch_latest_release()?,
    };
    let latest = parse_version_from_tag(&release.tag_name);

    if version.is_none() && !is_newer_version(current, latest) {
        println!("Already up to date ({current}).");
        return Ok(());
    }

    println!("Updating ceo {current} -> {latest}...");

    let asset_names: Vec<String> = release.assets.iter().map(|a| a.name.clone()).collect();
    let asset_name = match match_asset(&asset_names) {
        Some(name) => name,
        None => {
            println!("No prebuilt binary found for {}.", target_triple());
            println!("Available assets: {}", asset_names.join(", "));
            bail!("Install from source instead:\n  cargo install --git https://github.com/{REPO} --force");
        }
    };

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .expect("matched asset must exist in release");

    let tmp_dir = tempfile::tempdir()?;
    let archive_path = tmp_dir.path().join(&asset_name);
    println!("Downloading {asset_name}...");
    download_file(&asset.browser_download_url, &archive_path)?;

    let extract_dir = tmp_dir.path().join("extracted");
    extract_tar_gz(&archive_path, &extract_dir)?;

    let new_binary = find_binary_in_dir(&extract_dir)?;
    let installed_path = replace_exe(&new_binary)?;

    println!("Updated to ceo {latest}");
    println!("exe: {}", installed_path.display());
    Ok(())
}

// ── Background update hint ───────────────────────────────────────────

pub fn check_for_update_hint() {
    let _ = check_for_update_hint_inner();
}

fn update_check_cache_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("ceo").join("update_check.json"))
}

fn check_for_update_hint_inner() -> Option<()> {
    let cache_path = update_check_cache_path()?;
    let current = env!("CARGO_PKG_VERSION");

    // Check if we have a recent cached result
    if let Ok(contents) = std::fs::read_to_string(&cache_path)
        && let Ok(cached) = serde_json::from_str::<serde_json::Value>(&contents)
    {
        let ts = cached.get("timestamp")?.as_u64()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        if now.saturating_sub(ts) < UPDATE_CHECK_INTERVAL.as_secs() {
            let latest = cached.get("latest")?.as_str()?;
            if is_newer_version(current, latest) {
                print_update_hint(current, latest);
            }
            return Some(());
        }
    }

    // Cache is stale or missing — fetch from GitHub
    let release = fetch_latest_release().ok()?;
    let latest = parse_version_from_tag(&release.tag_name);

    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    let cache_value = serde_json::json!({
        "timestamp": now,
        "latest": latest,
    });
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache_path, cache_value.to_string());

    if is_newer_version(current, latest) {
        print_update_hint(current, latest);
    }

    Some(())
}

fn print_update_hint(current: &str, latest: &str) {
    eprintln!(
        "\nUpdate available: ceo {} -> {} — run `{}` to install",
        current, latest, update_command_hint()
    );
}
