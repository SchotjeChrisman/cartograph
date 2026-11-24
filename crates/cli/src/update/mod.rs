//! Keeps a CI-built binary on the newest build of the rolling `dev` prerelease. Every command
//! checks first; a build that is not from CI (no baked-in commit) never updates itself, and
//! `CARTOGRAPH_NO_UPDATE` switches the check off.
//!
//! The network and unpacking go through `curl` and `tar`, so the binary carries no HTTP stack.
//!
//! ponytail: no checksum or signature check beyond HTTPS to GitHub and a `--version` run of the
//! new binary; verify the release's `.sha256` asset if the update channel ever matters more.

mod release;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use crate::Result;

/// Set by CI to the commit the binary is built from.
const BUILT_FROM: Option<&str> = option_env!("CARTOGRAPH_BUILD_COMMIT");
const OPT_OUT_VARIABLE: &str = "CARTOGRAPH_NO_UPDATE";
const RELEASE_API: &str = "https://api.github.com/repos/SchotjeChrisman/cartograph/releases/tags/dev";
const CHECK_TIMEOUT_SECONDS: &str = "5";
const DOWNLOAD_TIMEOUT_SECONDS: &str = "120";
/// curl exit codes for no DNS, no connection and a timeout: offline is not worth a message.
const CURL_OFFLINE_CODES: [i32; 3] = [6, 7, 28];
/// Longer than any download, so a lock left by a killed process frees the update again.
const LOCK_LEASE: Duration = Duration::from_secs(600);
const SHORT_COMMIT_LENGTH: usize = 7;

const TARGET: &str = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    "x86_64-unknown-linux-musl"
} else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
    "aarch64-unknown-linux-musl"
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    "aarch64-apple-darwin"
} else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    "x86_64-apple-darwin"
} else {
    ""
};

/// Replaces the running binary when a newer build exists. Problems are reported on stderr and
/// never stop the command the user asked for.
pub fn update_if_newer() {
    let Some(current) = BUILT_FROM else { return };
    if std::env::var_os(OPT_OUT_VARIABLE).is_some() || TARGET.is_empty() {
        return;
    }
    match try_update(current) {
        Ok(Some(commit)) => eprintln!(
            "cartograph: updated to {}; it takes effect on the next run",
            &commit[..SHORT_COMMIT_LENGTH.min(commit.len())]
        ),
        Ok(None) => {}
        Err(error) => eprintln!("cartograph: update skipped: {error}"),
    }
}

/// The commit installed, or `None` when already current, offline or another process is updating.
fn try_update(current: &str) -> Result<Option<String>> {
    let Some(json) = fetch_release_json()? else {
        return Ok(None);
    };
    let Some(newest) = release::parse(&json, TARGET) else {
        return Err("the dev release has no build for this platform".into());
    };
    if newest.commit == current {
        return Ok(None);
    }
    let executable = std::env::current_exe()?;
    let Some(_lock) = Lock::acquire(&executable)? else {
        return Ok(None);
    };
    install(&newest, &executable)?;
    Ok(Some(newest.commit))
}

/// `None` when the network is unreachable.
fn fetch_release_json() -> Result<Option<String>> {
    let output = Command::new("curl")
        .args(["-fsSL", "--max-time", CHECK_TIMEOUT_SECONDS, RELEASE_API])
        .output()?;
    if output.status.success() {
        return Ok(Some(String::from_utf8(output.stdout)?));
    }
    if output.status.code().is_some_and(|code| CURL_OFFLINE_CODES.contains(&code)) {
        return Ok(None);
    }
    Err(format!("could not read the dev release: {}", String::from_utf8_lossy(&output.stderr).trim()).into())
}

/// Downloads and unpacks next to `executable`, so the final rename stays on one filesystem and
/// swaps the file atomically.
fn install(newest: &release::Release, executable: &Path) -> Result<()> {
    let directory = executable.parent().ok_or("the binary has no directory")?;
    let work = directory.join(format!(".cartograph-update-{}", std::process::id()));
    fs::create_dir_all(&work)?;
    let result = download_and_swap(newest, executable, &work);
    fs::remove_dir_all(&work)?;
    result
}

fn download_and_swap(newest: &release::Release, executable: &Path, work: &Path) -> Result<()> {
    let archive = work.join("build.tar.gz");
    run(Command::new("curl").args(["-fsSL", "--max-time", DOWNLOAD_TIMEOUT_SECONDS, "-o"]).arg(&archive).arg(&newest.download_url))?;
    run(Command::new("tar").arg("xzf").arg(&archive).arg("-C").arg(work))?;
    let name = release::archive_directory(&newest.download_url).ok_or("unexpected download name")?;
    let unpacked = work.join(name).join("cartograph");
    run(Command::new(&unpacked).arg("--version"))?;
    fs::rename(unpacked, executable)?;
    Ok(())
}

fn run(command: &mut Command) -> Result<()> {
    let output = command.output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!("{command:?} failed: {}", String::from_utf8_lossy(&output.stderr).trim()).into())
}

/// One updater at a time, so thirty commands starting together download once.
struct Lock(PathBuf);

impl Lock {
    fn acquire(executable: &Path) -> io::Result<Option<Lock>> {
        let path = executable.with_extension("update-lock");
        if is_stale(&path) {
            fs::remove_file(&path)?;
        }
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => Ok(Some(Lock(path))),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(None),
            Err(error) => Err(error),
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // A lock that cannot be removed expires on its own after LOCK_LEASE.
        let _ = fs::remove_file(&self.0);
    }
}

fn is_stale(lock: &Path) -> bool {
    fs::metadata(lock)
        .and_then(|metadata| metadata.modified())
        .and_then(|modified| SystemTime::now().duration_since(modified).map_err(io::Error::other))
        .is_ok_and(|age| age > LOCK_LEASE)
}
