//! Reading the rolling `dev` release: which commit it was built from and where this machine's
//! build can be downloaded. Pure: the caller fetches the JSON.

use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
pub struct Release {
    pub commit: String,
    pub download_url: String,
}

/// The release for `target`, or `None` when the JSON lacks a commit or a build for that target.
pub fn parse(release_json: &str, target: &str) -> Option<Release> {
    let release: Value = serde_json::from_str(release_json).ok()?;
    let commit = release["target_commitish"].as_str()?.to_string();
    let suffix = format!("-{target}.tar.gz");
    let download_url = release["assets"]
        .as_array()?
        .iter()
        .find(|asset| asset["name"].as_str().is_some_and(|name| name.ends_with(&suffix)))?["browser_download_url"]
        .as_str()?
        .to_string();
    Some(Release {
        commit,
        download_url,
    })
}

/// The directory name inside the archive: the archive's file name without `.tar.gz`.
pub fn archive_directory(download_url: &str) -> Option<&str> {
    download_url
        .rsplit('/')
        .next()?
        .strip_suffix(".tar.gz")
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{
        "target_commitish": "abc123",
        "assets": [
            {"name": "cartograph-dev-6-abc-x86_64-unknown-linux-musl.tar.gz", "browser_download_url": "https://example/linux.tar.gz"},
            {"name": "cartograph-dev-6-abc-x86_64-unknown-linux-musl.tar.gz.sha256", "browser_download_url": "https://example/linux.sha256"},
            {"name": "cartograph-dev-6-abc-aarch64-apple-darwin.tar.gz", "browser_download_url": "https://example/mac.tar.gz"}
        ]
    }"#;

    #[test]
    fn picks_the_archive_for_the_target_not_its_checksum() {
        let release = parse(JSON, "x86_64-unknown-linux-musl").unwrap();

        assert_eq!(release.commit, "abc123");
        assert_eq!(release.download_url, "https://example/linux.tar.gz");
    }

    #[test]
    fn a_target_without_a_build_gives_nothing() {
        assert_eq!(parse(JSON, "riscv64-unknown-linux-musl"), None);
        assert_eq!(parse("not json", "x86_64-unknown-linux-musl"), None);
    }

    #[test]
    fn the_archive_directory_drops_the_extension() {
        assert_eq!(
            archive_directory("https://example/a/cartograph-dev-6-abc-x.tar.gz"),
            Some("cartograph-dev-6-abc-x")
        );
    }
}
