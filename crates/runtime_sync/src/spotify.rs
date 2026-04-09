/// Spotify Web API client — minimal surface, just what the daemon needs.
///
/// Auth strategy: Authorization Code with PKCE.
///   1. `lightd auth` runs once (on any machine with a browser).
///   2. It saves access_token + refresh_token to a token file on disk.
///   3. The daemon loads the token file and auto-refreshes before expiry.
///
/// The daemon only calls two endpoints at runtime:
///   GET /me/player/currently-playing  (every poll_interval_secs)
///   POST /api/token                    (to refresh the access token)

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};

/// Minimal track info the daemon cares about.
#[derive(Debug, Clone)]
pub struct SpotifyTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: u32,
    pub progress_ms: u32,
    pub is_playing: bool,
    pub isrc: Option<String>,
}

/// Persisted OAuth token.
#[derive(Debug, Serialize, Deserialize)]
pub struct SpotifyAuth {
    pub access_token: String,
    pub refresh_token: String,
    /// When the access token expires (epoch secs — serialised as u64 for
    /// portability; re-derived from `expires_in` at auth time).
    pub expires_at_epoch_secs: u64,
    /// Stored at auth time so offline commands (e.g. beatmap-cli download)
    /// can refresh tokens without requiring --client-id every time.
    #[serde(default)]
    pub client_id: Option<String>,
}

impl SpotifyAuth {
    pub fn load(path: &Path) -> Result<Self> {
        let s = std::fs::read_to_string(path)
            .with_context(|| format!("reading token from {}", path.display()))?;
        serde_json::from_str(&s).context("parsing token file")
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let s = serde_json::to_string_pretty(self)?;
        std::fs::write(path, s).context("saving token file")
    }

    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // Refresh 60 s before actual expiry.
        now + 60 >= self.expires_at_epoch_secs
    }
}

/// HTTP client wrapping the Spotify API.
pub struct SpotifyClient {
    client: reqwest::Client,
    client_id: String,
    auth: SpotifyAuth,
    token_path: std::path::PathBuf,
    last_request: Option<Instant>,
    /// Minimum interval between requests to avoid rate-limiting (enforced by caller).
    #[allow(dead_code)]
    min_interval: Duration,
}

impl SpotifyClient {
    pub fn new(
        client_id: String,
        auth: SpotifyAuth,
        token_path: impl AsRef<Path>,
        poll_interval: Duration,
    ) -> Self {
        SpotifyClient {
            client: reqwest::Client::new(),
            client_id,
            auth,
            token_path: token_path.as_ref().to_owned(),
            last_request: None,
            min_interval: poll_interval,
        }
    }

    /// Refresh the access token if needed. Call before any API request.
    pub async fn maybe_refresh(&mut self) -> Result<()> {
        if !self.auth.is_expired() {
            return Ok(());
        }
        tracing::debug!("refreshing Spotify access token");

        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", &self.auth.refresh_token),
            ("client_id", &self.client_id),
        ];

        let resp: serde_json::Value = self
            .client
            .post("https://accounts.spotify.com/api/token")
            .form(&params)
            .send()
            .await
            .context("token refresh request")?
            .error_for_status()
            .context("token refresh HTTP error")?
            .json()
            .await
            .context("parsing token response")?;

        let access_token = resp["access_token"]
            .as_str()
            .context("missing access_token")?
            .to_owned();
        let expires_in = resp["expires_in"].as_u64().unwrap_or(3600);

        // Some refresh responses include a new refresh token; keep it if present.
        if let Some(rt) = resp["refresh_token"].as_str() {
            self.auth.refresh_token = rt.to_owned();
        }

        self.auth.access_token = access_token;
        self.auth.expires_at_epoch_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + expires_in;

        self.auth.save(&self.token_path)?;
        Ok(())
    }

    /// Fetch static track info by Spotify track ID.
    /// Useful for offline commands that need ISRC/metadata without polling playback.
    pub async fn track_by_id(&mut self, id: &str) -> Result<SpotifyTrack> {
        self.maybe_refresh().await?;

        let url = format!("https://api.spotify.com/v1/tracks/{}", id);
        let body: serde_json::Value = self
            .client
            .get(&url)
            .bearer_auth(&self.auth.access_token)
            .send()
            .await
            .context("track lookup request")?
            .error_for_status()
            .context("track lookup HTTP error")?
            .json()
            .await
            .context("parsing track response")?;

        let title = body["name"].as_str().unwrap_or("Unknown").to_owned();
        let artist = body["artists"][0]["name"]
            .as_str()
            .unwrap_or("Unknown")
            .to_owned();
        let duration_ms = body["duration_ms"].as_u64().unwrap_or(0) as u32;
        let isrc = body["external_ids"]["isrc"]
            .as_str()
            .map(|s| s.to_owned());

        Ok(SpotifyTrack {
            id: id.to_owned(),
            title,
            artist,
            duration_ms,
            progress_ms: 0,
            is_playing: false,
            isrc,
        })
    }

    /// Poll the currently playing track.
    /// Returns None if nothing is playing or the player is in a non-music context.
    pub async fn current_track(&mut self) -> Result<Option<SpotifyTrack>> {
        self.maybe_refresh().await?;

        let resp = self
            .client
            .get("https://api.spotify.com/v1/me/player/currently-playing")
            .bearer_auth(&self.auth.access_token)
            .send()
            .await
            .context("currently-playing request")?;

        // 204 = nothing playing.
        if resp.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(None);
        }

        let resp = resp.error_for_status().context("API error")?;
        let body: serde_json::Value = resp.json().await.context("parsing response")?;

        // Ignore podcast episodes and ads.
        if body["currently_playing_type"].as_str() != Some("track") {
            return Ok(None);
        }

        let item = &body["item"];
        let id = item["id"].as_str().context("missing track id")?.to_owned();
        let title = item["name"].as_str().unwrap_or("Unknown").to_owned();
        let artist = item["artists"][0]["name"]
            .as_str()
            .unwrap_or("Unknown")
            .to_owned();
        let duration_ms = item["duration_ms"].as_u64().unwrap_or(0) as u32;
        let progress_ms = body["progress_ms"].as_u64().unwrap_or(0) as u32;
        let is_playing = body["is_playing"].as_bool().unwrap_or(false);
        let isrc = item["external_ids"]["isrc"]
            .as_str()
            .map(|s| s.to_owned());

        self.last_request = Some(Instant::now());

        Ok(Some(SpotifyTrack {
            id,
            title,
            artist,
            duration_ms,
            progress_ms,
            is_playing,
            isrc,
        }))
    }
}

// ─── One-time auth flow ───────────────────────────────────────────────────────

/// Run the PKCE authorization code flow and return a token.
///
/// Call this from `lightd auth` or `beatmap-cli auth`.
/// Starts a temporary local HTTP server to receive the callback.
pub async fn run_auth_flow(client_id: &str, port: u16) -> Result<SpotifyAuth> {
    // Generate code verifier (random 96-byte base64url string).
    let verifier = generate_code_verifier();
    let challenge = code_challenge(&verifier);

    let redirect_uri = format!("http://localhost:{port}/callback");
    let scopes = "user-read-playback-state user-read-currently-playing";

    let auth_url = format!(
        "https://accounts.spotify.com/authorize\
        ?client_id={client_id}\
        &response_type=code\
        &redirect_uri={redirect_uri}\
        &scope={scopes}\
        &code_challenge_method=S256\
        &code_challenge={challenge}",
        redirect_uri = urlencoding(&redirect_uri),
        scopes = urlencoding(scopes),
    );

    println!("Open this URL in your browser:\n\n  {auth_url}\n");

    // Receive the callback code via a minimal HTTP listener.
    let code = receive_auth_code(port).await?;

    // Exchange code for tokens.
    let client = reqwest::Client::new();
    let params = [
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", &redirect_uri),
        ("client_id", client_id),
        ("code_verifier", &verifier),
    ];

    let resp: serde_json::Value = client
        .post("https://accounts.spotify.com/api/token")
        .form(&params)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let access_token = resp["access_token"].as_str().context("no access_token")?.to_owned();
    let refresh_token = resp["refresh_token"].as_str().context("no refresh_token")?.to_owned();
    let expires_in = resp["expires_in"].as_u64().unwrap_or(3600);
    let expires_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + expires_in;

    Ok(SpotifyAuth { access_token, refresh_token, expires_at_epoch_secs: expires_at, client_id: Some(client_id.to_owned()) })
}

/// Listen on `localhost:port` and extract the `code` query parameter.
async fn receive_auth_code(port: u16) -> Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .context("binding local auth server")?;

    println!("Waiting for Spotify redirect on port {port}...");
    let (mut stream, _) = listener.accept().await?;

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse the GET line: "GET /callback?code=<CODE>&... HTTP/1.1"
    let code = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|path| {
            path.split('?')
                .nth(1)
                .and_then(|qs| {
                    qs.split('&')
                        .find(|p| p.starts_with("code="))
                        .map(|p| p.trim_start_matches("code=").to_owned())
                })
        })
        .context("no code in callback URL")?;

    // Send a minimal success response.
    stream
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nAuth complete. Return to terminal.")
        .await?;

    Ok(code)
}

fn generate_code_verifier() -> String {
    // Use urandom for a 96-byte verifier (→ 128-char base64url).
    let mut bytes = [0u8; 72];
    getrandom(&mut bytes);
    base64url_encode(&bytes)
}

fn code_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(verifier.as_bytes());
    base64url_encode(&hash)
}

fn base64url_encode(input: &[u8]) -> String {
    // Simple base64url without padding.
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut i = 0;
    let len = input.len();
    while i < len {
        let b0 = input[i] as usize;
        let b1 = if i + 1 < len { input[i + 1] as usize } else { 0 };
        let b2 = if i + 2 < len { input[i + 2] as usize } else { 0 };
        out.push(TABLE[b0 >> 2] as char);
        out.push(TABLE[((b0 & 3) << 4) | (b1 >> 4)] as char);
        if i + 1 < len { out.push(TABLE[((b1 & 0xf) << 2) | (b2 >> 6)] as char); }
        if i + 2 < len { out.push(TABLE[b2 & 0x3f] as char); }
        i += 3;
    }
    out
}

fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn getrandom(buf: &mut [u8]) {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(buf))
        .expect("failed to read /dev/urandom");
}
