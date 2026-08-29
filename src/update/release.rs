use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

const DEFAULT_API_BASE: &str = "https://api.github.com";
const STALL_TIMEOUT: Duration = Duration::from_secs(30);
const USER_AGENT: &str = concat!("slack-cli/", env!("CARGO_PKG_VERSION"));

/// The GitHub repository this binary was published from, taken from the
/// manifest so the updater and the crate metadata cannot name two places.
pub fn repository() -> Result<String> {
    let url = env!("CARGO_PKG_REPOSITORY");
    url.trim_end_matches('/')
        .trim_end_matches(".git")
        .split("github.com/")
        .nth(1)
        .filter(|slug| slug.split('/').count() == 2)
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("package repository {url} is not a GitHub project"))
}

#[derive(Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

impl Release {
    pub fn version(&self) -> &str {
        self.tag_name.trim_start_matches('v')
    }

    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.name == name)
    }

    pub fn require(&self, name: &str) -> Result<&Asset> {
        self.asset(name).ok_or_else(|| {
            anyhow!(
                "release {} publishes no asset named {name}. \
                 reinstall with scripts/install.sh instead",
                self.tag_name
            )
        })
    }
}

pub struct Releases {
    api_base: String,
    repository: String,
    http: reqwest::Client,
}

impl Releases {
    pub fn new(repository: String) -> Result<Self> {
        Self::with_api_base(repository, DEFAULT_API_BASE.to_string())
    }

    pub fn with_api_base(repository: String, api_base: String) -> Result<Self> {
        Ok(Self {
            api_base: api_base.trim_end_matches('/').to_string(),
            repository,
            http: reqwest::Client::builder()
                .read_timeout(STALL_TIMEOUT)
                .user_agent(USER_AGENT)
                .build()
                .context("failed to create the update HTTP client")?,
        })
    }

    pub async fn latest(&self) -> Result<Release> {
        self.fetch(&format!(
            "{}/repos/{}/releases/latest",
            self.api_base, self.repository
        ))
        .await
    }

    pub async fn tagged(&self, version: &str) -> Result<Release> {
        let tag = format!("v{}", version.trim_start_matches('v'));
        self.fetch(&format!(
            "{}/repos/{}/releases/tags/{tag}",
            self.api_base, self.repository
        ))
        .await
    }

    async fn fetch(&self, url: &str) -> Result<Release> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .with_context(|| format!("could not reach {url}"))?;
        let status = response.status();
        if !status.is_success() {
            bail!("GitHub returned {status} for {url}");
        }
        response
            .json()
            .await
            .context("GitHub's release response did not match the expected shape")
    }

    pub async fn download(&self, asset: &Asset, into: &Path) -> Result<PathBuf> {
        let response = self
            .http
            .get(&asset.browser_download_url)
            .send()
            .await
            .with_context(|| format!("could not download {}", asset.name))?;
        let status = response.status();
        if !status.is_success() {
            bail!("downloading {} returned {status}", asset.name);
        }
        let bytes = response
            .bytes()
            .await
            .with_context(|| format!("could not read {}", asset.name))?;

        let path = into.join(&asset.name);
        std::fs::write(&path, &bytes)
            .with_context(|| format!("could not write {}", path.display()))?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(assets: &[&str]) -> Release {
        Release {
            tag_name: "v0.9.0".into(),
            assets: assets
                .iter()
                .map(|name| Asset {
                    name: (*name).to_string(),
                    browser_download_url: format!("https://example.test/{name}"),
                })
                .collect(),
        }
    }

    #[test]
    fn the_repository_slug_comes_from_the_manifest() {
        assert_eq!(repository().unwrap(), "junyeong-ai/slack-cli");
    }

    #[test]
    fn the_tag_is_reported_without_its_leading_v() {
        assert_eq!(release(&[]).version(), "0.9.0");
    }

    #[test]
    fn a_missing_asset_names_the_release_and_the_way_out() {
        let err = release(&["other"]).require("wanted").unwrap_err();
        let message = err.to_string();
        assert!(message.contains("v0.9.0"), "{message}");
        assert!(message.contains("wanted"), "{message}");
        assert!(message.contains("install.sh"), "{message}");
    }

    #[test]
    fn assets_are_matched_by_exact_name() {
        let release = release(&["slack-cli-v0.9.0-x86_64-apple-darwin"]);
        assert!(
            release
                .asset("slack-cli-v0.9.0-x86_64-apple-darwin")
                .is_some()
        );
        assert!(
            release
                .asset("slack-cli-v0.9.0-x86_64-apple-darwin.exe")
                .is_none()
        );
    }
}
