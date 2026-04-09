/// Deezer public API — no auth required.
///
/// Used offline to resolve an ISRC to a Deezer track ID so that streamrip
/// receives an exact-match URL rather than relying on its own Spotify→Deezer
/// heuristic search.

use anyhow::{bail, Context, Result};

/// Look up a Deezer track ID by ISRC.
///
/// Endpoint: GET https://api.deezer.com/track/isrc:{isrc}
/// Returns the Deezer track ID (u64) on success.
pub async fn track_id_by_isrc(isrc: &str) -> Result<u64> {
    let url = format!("https://api.deezer.com/track/isrc:{isrc}");
    let body: serde_json::Value = reqwest::get(&url)
        .await
        .context("Deezer ISRC lookup request failed")?
        .error_for_status()
        .context("Deezer ISRC lookup HTTP error")?
        .json()
        .await
        .context("parsing Deezer ISRC response")?;

    if let Some(err) = body.get("error") {
        bail!("Deezer API error: {err}");
    }

    body["id"]
        .as_u64()
        .context("Deezer response missing 'id' field")
}

/// Build the canonical Deezer track URL from an ID.
pub fn track_url(deezer_id: u64) -> String {
    format!("https://www.deezer.com/track/{deezer_id}")
}
