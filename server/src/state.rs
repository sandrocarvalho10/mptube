use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Mutex;

use tokio::sync::{broadcast, Semaphore};

use mptube_core::DownloadProgress;

pub const DEFAULT_ALLOWED_DOMAINS: &[&str] = &[
    "youtube.com",
    "youtu.be",
    "instagram.com",
    "tiktok.com",
    "twitter.com",
    "x.com",
    "facebook.com",
    "fb.watch",
    "vimeo.com",
    "soundcloud.com",
];

pub struct Config {
    pub port: u16,
    pub download_dir: PathBuf,
    pub frontend_dist: PathBuf,
    pub ytdlp_bin: String,
    pub ffmpeg_bin: Option<String>,
    pub retention_minutes: u64,
    pub max_concurrent_downloads: usize,
    pub max_concurrent_per_ip: usize,
    pub rate_limit_per_minute: u64,
    /// `None` = qualquer domínio é aceito (uso interno/confiável).
    pub allowed_domains: Option<Vec<String>>,
    pub min_free_disk_mb: u64,
}

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

impl Config {
    pub fn from_env() -> Self {
        let allow_any = std::env::var("ALLOW_ANY_DOMAIN")
            .map(|v| v == "true")
            .unwrap_or(false);

        let allowed_domains = if allow_any {
            None
        } else {
            let raw = std::env::var("ALLOWED_DOMAINS")
                .unwrap_or_else(|_| DEFAULT_ALLOWED_DOMAINS.join(","));
            Some(
                raw.split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect(),
            )
        };

        Self {
            port: env_or("PORT", 8080),
            download_dir: PathBuf::from(
                std::env::var("DOWNLOAD_DIR").unwrap_or_else(|_| "/data/downloads".to_string()),
            ),
            frontend_dist: PathBuf::from(
                std::env::var("FRONTEND_DIST").unwrap_or_else(|_| "dist".to_string()),
            ),
            ytdlp_bin: std::env::var("YTDLP_BIN").unwrap_or_else(|_| "yt-dlp".to_string()),
            ffmpeg_bin: std::env::var("FFMPEG_BIN").ok(),
            retention_minutes: env_or("RETENTION_MINUTES", 60),
            max_concurrent_downloads: env_or("MAX_CONCURRENT_DOWNLOADS", 4),
            max_concurrent_per_ip: env_or("MAX_CONCURRENT_PER_IP", 2),
            rate_limit_per_minute: env_or("RATE_LIMIT_PER_MINUTE", 20),
            allowed_domains,
            min_free_disk_mb: env_or("MIN_FREE_DISK_MB", 500),
        }
    }
}

pub struct AppState {
    pub config: Config,
    pub semaphore: std::sync::Arc<Semaphore>,
    pub per_ip: Mutex<HashMap<IpAddr, usize>>,
    /// Handle da task + IP que a iniciou (para decrementar `per_ip` ao cancelar).
    pub handles: Mutex<HashMap<String, (tokio::task::JoinHandle<()>, IpAddr)>>,
    pub completed_files: Mutex<HashMap<String, PathBuf>>,
    pub progress_tx: broadcast::Sender<DownloadProgress>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let (progress_tx, _rx) = broadcast::channel(512);
        Self {
            semaphore: std::sync::Arc::new(Semaphore::new(config.max_concurrent_downloads)),
            per_ip: Mutex::new(HashMap::new()),
            handles: Mutex::new(HashMap::new()),
            completed_files: Mutex::new(HashMap::new()),
            progress_tx,
            config,
        }
    }

    pub fn decrement_ip(&self, ip: IpAddr) {
        let mut per_ip = self.per_ip.lock().unwrap();
        if let Some(count) = per_ip.get_mut(&ip) {
            if *count > 0 {
                *count -= 1;
            }
            if *count == 0 {
                per_ip.remove(&ip);
            }
        }
    }
}
