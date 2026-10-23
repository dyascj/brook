//! Self-update from GitHub releases, the same flow as Slat and Glint.
//!
//! A check asks GitHub for the latest release. When it is newer and carries
//! a signed build for this platform, the app offers it. Installing downloads
//! the build, checks its minisign signature against the key built into the
//! app (including the trusted comment, so an older signed build can't pass
//! for a newer one), swaps it in place, and relaunches.
//!
//! - macOS: the release asset is `Brook.app` in a `.tar.gz`; the running
//!   bundle is renamed aside and the new one renamed into its place.
//! - Linux: the asset holds the `brook` binary; it replaces the running
//!   executable, which Linux allows while it runs.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use crate::error::{Error, Result};
use crate::http::Client;

/// This build's version.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Where releases are published.
pub const RELEASES_URL: &str = "https://github.com/dyascj/cozy-rss/releases/latest";

const FEED: &str = "https://api.github.com/repos/dyascj/cozy-rss/releases/latest";

/// Release builds are signed in CI with the matching secret key.
pub const PUBLIC_KEY: &str = "RWR/D8rYwr7IIakiLumdMumb2LWFXskkdgcMCY4ZVxakZ+CrCCTlPCuX";

/// A newer release with a signed build for this platform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// Its version, without the `v`.
    pub version: String,
    /// The build's download address.
    pub url: String,
    /// The signature's download address.
    pub signature_url: String,
}

/// The release asset for this platform, if Brook ships one.
pub fn asset_name() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("Brook-macos.app.tar.gz")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("Brook-linux-x86_64.tar.gz")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("Brook-linux-aarch64.tar.gz")
    } else {
        None
    }
}

/// Why this copy can't update itself, if it can't. Development builds,
/// `BROOK_NO_UPDATE`, and copies that aren't installed the way releases
/// install never self-update.
pub fn disabled_reason() -> Option<&'static str> {
    if cfg!(debug_assertions) {
        return Some("development builds don't update themselves");
    }
    if env::var_os("BROOK_NO_UPDATE").is_some() {
        return Some("BROOK_NO_UPDATE is set");
    }
    if asset_name().is_none() {
        return Some("there are no builds for this platform");
    }
    match install_target() {
        Ok(_) => None,
        Err(_) if cfg!(target_os = "macos") => Some("Brook isn't running from Brook.app"),
        Err(_) => Some("Brook can't write to the folder it runs from"),
    }
}

/// Whether release tag `tag` ("v1.2.0") is a later version than `current`.
/// Pre-release and non-numeric tags never count.
pub fn is_newer(tag: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v').split(['.', '-', '+']).map_while(|part| part.parse().ok()).collect()
    };
    let (theirs, ours) = (parse(tag), parse(current));
    let is_release = !tag.trim_start_matches('v').contains('-');
    is_release && !theirs.is_empty() && theirs > ours
}

/// Reads the newest release from GitHub's answer, if it is newer than
/// `current` and has a signed `asset`.
pub fn pick_release(json: &str, current: &str, asset: &str) -> Result<Option<Release>> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| Error::Network(format!("unexpected answer: {error}")))?;
    let tag = value["tag_name"].as_str().unwrap_or_default();
    if !is_newer(tag, current) {
        return Ok(None);
    }
    let url_of = |name: &str| {
        value["assets"].as_array()?.iter().find(|item| item["name"] == name)?["browser_download_url"]
            .as_str()
            .map(str::to_owned)
    };
    Ok(match (url_of(asset), url_of(&format!("{asset}.minisig"))) {
        (Some(url), Some(signature_url)) => {
            Some(Release { version: tag.trim_start_matches('v').to_owned(), url, signature_url })
        }
        _ => None,
    })
}

/// The latest release, when it is newer than this build.
pub fn check(client: &Client) -> Result<Option<Release>> {
    let Some(asset) = asset_name() else { return Ok(None) };
    match client.get(FEED, "application/vnd.github+json") {
        Ok(page) => pick_release(&page.text(), CURRENT, asset),
        // GitHub answers 404 while the repository has no releases.
        Err(Error::Status { status: 404 }) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The trusted comment CI signs each build with.
pub fn signed_comment(version: &str, asset: &str) -> String {
    format!("Brook v{version} {asset}")
}

/// Whether `signature` (a .minisig file's text) is a valid signature of
/// `data` by `key` whose trusted comment is exactly `comment`.
pub fn verify(key: &str, data: &[u8], signature: &str, comment: &str) -> bool {
    let Ok(key) = minisign_verify::PublicKey::from_base64(key) else { return false };
    let Ok(signature) = minisign_verify::Signature::decode(signature) else { return false };
    key.verify(data, &signature, false).is_ok() && signature.trusted_comment() == comment
}

/// What gets replaced: the running `.app` on macOS, the executable on Linux.
fn install_target() -> Result<PathBuf> {
    let exe = env::current_exe()?;
    if cfg!(target_os = "macos") {
        let bundle = exe.ancestors().nth(3).ok_or(Error::NotFound("app bundle"))?;
        if bundle.extension().is_none_or(|ext| ext != "app") {
            return Err(Error::NotFound("app bundle"));
        }
        Ok(bundle.to_path_buf())
    } else {
        let exe = exe.canonicalize()?;
        let dir = exe.parent().ok_or(Error::NotFound("install folder"))?;
        // Writable means we can stage and rename next to it.
        let probe = dir.join(".brook-write-test");
        fs::write(&probe, b"")?;
        let _ = fs::remove_file(&probe);
        Ok(exe)
    }
}

fn remove(path: &Path) {
    let _ = if path.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) };
}

/// Downloads, verifies, and installs `release`. Brook keeps running the old
/// code until [`relaunch`].
pub fn install(client: &Client, release: &Release) -> Result<()> {
    let asset = asset_name().ok_or(Error::NotFound("build for this platform"))?;
    let data = client.get(&release.url, "application/octet-stream")?.body;
    let signature = client.get(&release.signature_url, "text/plain")?.text();
    if !verify(PUBLIC_KEY, &data, &signature, &signed_comment(&release.version, asset)) {
        return Err(Error::Network("the download's signature doesn't match".into()));
    }

    let target = install_target()?;
    let parent = target.parent().ok_or(Error::NotFound("install folder"))?;
    // Unpack next to the target so the swap is a rename on one disk.
    let stage = parent.join(".brook-update");
    remove(&stage);
    fs::create_dir_all(&stage)?;
    let archive = stage.join(asset);
    fs::write(&archive, &data)?;
    let unpacked = Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&stage)
        .status()
        .is_ok_and(|status| status.success());
    let fresh = stage.join(if cfg!(target_os = "macos") { "Brook.app" } else { "brook/brook" });
    if !unpacked || !fresh.exists() {
        remove(&stage);
        return Err(Error::Io(std::io::Error::other(if unpacked {
            "the download is missing the app"
        } else {
            "couldn't unpack the download"
        })));
    }

    // Move the running copy aside (allowed while it runs), put the new one
    // in its place, and put the old one back if that fails.
    let old = parent.join(if cfg!(target_os = "macos") { ".Brook-old.app" } else { ".brook-old" });
    remove(&old);
    fs::rename(&target, &old)?;
    if let Err(error) = fs::rename(&fresh, &target) {
        let _ = fs::rename(&old, &target);
        remove(&stage);
        return Err(error.into());
    }
    remove(&old);
    remove(&stage);
    Ok(())
}

/// Starts the installed copy once this process exits. The caller quits
/// right after.
pub fn relaunch() -> Result<()> {
    let target = install_target()?;
    let quoted = target.display().to_string().replace('\'', "'\\''");
    let open =
        if cfg!(target_os = "macos") { format!("open -n '{quoted}'") } else { format!("'{quoted}' &") };
    let script = format!("while kill -0 {} 2>/dev/null; do sleep 0.2; done; {open}", std::process::id());
    Command::new("/bin/sh").args(["-c", &script]).spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("v0.1.1", "0.1.0"));
        assert!(is_newer("v1.10.0", "1.9.9"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(!is_newer("v1.0.0", "1.0.0"));
        assert!(!is_newer("v0.9.0", "1.0.0"));
        assert!(!is_newer("v2.0.0-beta.2", "1.0.0"), "pre-releases never count");
        assert!(!is_newer("nightly", "1.0.0"));
    }

    const ANSWER: &str = r#"{
        "tag_name": "v9.9.9",
        "assets": [
            {"name": "Brook-macos.app.tar.gz", "browser_download_url": "https://dl.test/a.tar.gz"},
            {"name": "Brook-macos.app.tar.gz.minisig", "browser_download_url": "https://dl.test/a.tar.gz.minisig"},
            {"name": "Brook-linux-x86_64.tar.gz", "browser_download_url": "https://dl.test/l.tar.gz"}
        ]
    }"#;

    #[test]
    fn picks_signed_newer_builds() {
        let release = pick_release(ANSWER, "0.1.0", "Brook-macos.app.tar.gz").unwrap().unwrap();
        assert_eq!(release.version, "9.9.9");
        assert_eq!(release.url, "https://dl.test/a.tar.gz");
        assert_eq!(release.signature_url, "https://dl.test/a.tar.gz.minisig");

        assert_eq!(pick_release(ANSWER, "0.1.0", "Brook-linux-x86_64.tar.gz").unwrap(), None, "unsigned");
        assert_eq!(pick_release(ANSWER, "9.9.9", "Brook-macos.app.tar.gz").unwrap(), None, "not newer");
        assert!(pick_release("not json", "0.1.0", "x").is_err());
    }

    // A throwaway key pair, generated with `minisign -G -W`; the secret was
    // discarded. The signature covers b"hello brook\n".
    const TEST_KEY: &str = "RWQ1VZiXw5UX7my94WmOtKl/NfeoK6NybicuwjjFQSvW0T4rSIfZDtyi";
    const TEST_SIGNATURE: &str = "untrusted comment: signature from minisign secret key
RUQ1VZiXw5UX7ltYYxPIsnT3WN81XCQWtGfM0nDLhuGy6E3hisj/tAcrBuT0Sp6/AcDDGYFMicqsN9PGmWw6HWrfapyt3K5h8wE=
trusted comment: Brook v9.9.9 Brook-macos.app.tar.gz
VLCQKSfiLhdbT66hxs32Wn7yDN1jlODbWMzi38FJvOA4XMA0nRkoesjMdneF5SuTzqH8+/Zy2Kc2lYY/lFQKBA==
";

    #[test]
    fn only_correctly_signed_downloads_verify() {
        let comment = signed_comment("9.9.9", "Brook-macos.app.tar.gz");
        assert!(verify(TEST_KEY, b"hello brook\n", TEST_SIGNATURE, &comment));
        assert!(!verify(TEST_KEY, b"hello brook!\n", TEST_SIGNATURE, &comment), "a changed download fails");
        let older = signed_comment("9.9.8", "Brook-macos.app.tar.gz");
        assert!(
            !verify(TEST_KEY, b"hello brook\n", TEST_SIGNATURE, &older),
            "another version's signature fails"
        );
        assert!(
            !verify(PUBLIC_KEY, b"hello brook\n", TEST_SIGNATURE, &comment),
            "another key's signature fails"
        );
        assert!(!verify(TEST_KEY, b"hello brook\n", "garbage", &comment));
    }

    #[test]
    fn the_built_in_key_parses() {
        assert!(minisign_verify::PublicKey::from_base64(PUBLIC_KEY).is_ok());
    }

    #[test]
    fn signed_comments_name_version_and_asset() {
        assert_eq!(signed_comment("1.2.3", "Brook-macos.app.tar.gz"), "Brook v1.2.3 Brook-macos.app.tar.gz");
    }

    #[test]
    fn debug_builds_never_update() {
        assert!(disabled_reason().is_some());
    }
}
