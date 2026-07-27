use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Release {
    pub tag: String,
    pub date: String,
    pub image_url: String,
    pub image_checksum: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Error {}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    published_at: String,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub async fn fetch_releases(repo: &str) -> Result<Vec<Release>, Error> {
    let url = format!("https://api.github.com/repos/{repo}/releases");
    let client = reqwest::Client::builder()
        .user_agent("caustic-installer")
        .build()
        .map_err(|e| Error(e.to_string()))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| Error(e.to_string()))?;

    if !response.status().is_success() {
        return Err(Error(format!("GitHub API returned {}", response.status())));
    }

    let releases: Vec<GithubRelease> =
        response.json().await.map_err(|e| Error(e.to_string()))?;

    let result = releases
        .into_iter()
        .filter_map(|r| {
            let image_asset = r
                .assets
                .iter()
                .find(|a| a.name.ends_with(".img.xz"))?;

            let checksum_asset = r
                .assets
                .iter()
                .find(|a| a.name == "SHA256SUMS");

            Some(Release {
                tag: r.tag_name,
                date: r.published_at.split('T').next().unwrap_or("").to_string(),
                image_url: image_asset.browser_download_url.clone(),
                image_checksum: checksum_asset.map(|a| a.browser_download_url.clone()),
            })
        })
        .collect();

    Ok(result)
}
