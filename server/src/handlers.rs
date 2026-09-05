use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use mptube_core::DownloadProgress;
use tokio_util::io::ReaderStream;

use crate::domains::url_allowed;
use crate::state::AppState;

type ApiError = (StatusCode, Json<serde_json::Value>);

fn err(status: StatusCode, msg: impl Into<String>) -> ApiError {
    (status, Json(serde_json::json!({ "error": msg.into() })))
}

/// Extrai o IP do cliente a partir de X-Forwarded-For / X-Real-IP (setados pelo nginx),
/// com fallback para o IP da conexão TCP direta.
fn client_ip(headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
    if let Some(v) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = v.split(',').next() {
            if let Ok(ip) = first.trim().parse::<IpAddr>() {
                return ip;
            }
        }
    }
    if let Some(v) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        if let Ok(ip) = v.trim().parse::<IpAddr>() {
            return ip;
        }
    }
    peer.ip()
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn enough_disk_space(dir: &std::path::Path, min_free_mb: u64) -> bool {
    match fs2::available_space(dir) {
        Ok(bytes) => bytes >= min_free_mb * 1024 * 1024,
        Err(_) => true, // não bloqueia se não conseguir checar (ex: path ainda não montado)
    }
}

/// Sanitiza um nome de arquivo para uso seguro em `Content-Disposition` (ASCII only).
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.trim().is_empty() {
        "download".to_string()
    } else {
        cleaned
    }
}

// ── DTOs ─────────────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct FormatsRequest {
    pub url: String,
}

#[derive(serde::Deserialize)]
pub struct StartDownloadRequest {
    pub id: String,
    pub url: String,
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub format: String,
    pub quality: String,
}

const UNSUPPORTED_DOMAIN_MSG: &str = "Este servidor só aceita links de YouTube, Instagram, TikTok, Twitter/X, Facebook, Vimeo e SoundCloud";

// ── Handlers ─────────────────────────────────────────────────────────────────

pub async fn formats_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<FormatsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !url_allowed(&body.url, &state.config.allowed_domains) {
        return Err(err(StatusCode::FORBIDDEN, UNSUPPORTED_DOMAIN_MSG));
    }

    mptube_core::fetch_formats_json(
        &state.config.ytdlp_bin,
        state.config.ffmpeg_bin.as_deref(),
        None,
        &body.url,
    )
    .await
    .map(Json)
    .map_err(|e| err(StatusCode::BAD_GATEWAY, e))
}

pub async fn start_download_handler(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<StartDownloadRequest>,
) -> Result<StatusCode, ApiError> {
    if !valid_id(&body.id) {
        return Err(err(StatusCode::BAD_REQUEST, "id inválido"));
    }
    if !url_allowed(&body.url, &state.config.allowed_domains) {
        return Err(err(StatusCode::FORBIDDEN, UNSUPPORTED_DOMAIN_MSG));
    }

    let ip = client_ip(&headers, addr);

    {
        let mut per_ip = state.per_ip.lock().unwrap();
        let count = per_ip.entry(ip).or_insert(0);
        if *count >= state.config.max_concurrent_per_ip {
            return Err(err(
                StatusCode::TOO_MANY_REQUESTS,
                "Você já tem downloads em andamento — aguarde um terminar antes de iniciar outro",
            ));
        }
        *count += 1;
    }

    if !enough_disk_space(&state.config.download_dir, state.config.min_free_disk_mb) {
        state.decrement_ip(ip);
        return Err(err(
            StatusCode::INSUFFICIENT_STORAGE,
            "Servidor sem espaço em disco no momento — tente novamente mais tarde",
        ));
    }

    let download_dir = state.config.download_dir.join(&body.id);
    if let Err(e) = tokio::fs::create_dir_all(&download_dir).await {
        state.decrement_ip(ip);
        return Err(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Erro ao preparar pasta de download: {e}"),
        ));
    }

    let (format_arg, format_sort) = mptube_core::build_format_arg(&body.media_type, &body.quality);
    let bin_path = state.config.ytdlp_bin.clone();
    let ffmpeg_path = state.config.ffmpeg_bin.clone();
    let broadcast_tx = state.progress_tx.clone();
    let id = body.id.clone();
    let url = body.url.clone();
    let media_type = body.media_type.clone();
    let format = body.format.clone();
    let download_dir_str = download_dir.to_string_lossy().to_string();
    let semaphore = Arc::clone(&state.semaphore);
    let state_for_task = Arc::clone(&state);

    let _ = broadcast_tx.send(DownloadProgress {
        id: id.clone(),
        progress: 0.0,
        speed: None,
        eta: None,
        status: "fetching_info".to_string(),
        title: None,
        file_path: None,
        error_message: None,
        attempt: None,
        max_attempts: None,
    });

    let handle = tokio::spawn(async move {
        // `run_ytdlp_with_retry` fala em um canal mpsc (mesma interface usada pelo
        // desktop); aqui um forwarder republica cada evento no broadcast channel do
        // servidor, que alimenta todas as conexões WebSocket abertas.
        let (progress_tx, mut progress_rx) =
            tokio::sync::mpsc::unbounded_channel::<DownloadProgress>();
        let broadcast_for_forward = broadcast_tx.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(p) = progress_rx.recv().await {
                let _ = broadcast_for_forward.send(p);
            }
        });

        let _permit = semaphore.acquire_owned().await;

        let build_cmd = || {
            mptube_core::build_ytdlp_cmd(
                &bin_path,
                &url,
                &media_type,
                &format,
                &format_arg,
                format_sort.as_deref(),
                &download_dir_str,
                None,
                ffmpeg_path.as_deref(),
            )
        };

        let (success, stderr, file_path) = mptube_core::run_ytdlp_with_retry(
            build_cmd,
            &id,
            &progress_tx,
            mptube_core::RetryConfig::default(),
        )
        .await;

        drop(progress_tx);
        let _ = forwarder.await;

        state_for_task.handles.lock().unwrap().remove(&id);
        state_for_task.decrement_ip(ip);

        if success {
            if let Some(fp) = &file_path {
                state_for_task
                    .completed_files
                    .lock()
                    .unwrap()
                    .insert(id.clone(), PathBuf::from(fp));
            }
            let _ = broadcast_tx.send(DownloadProgress {
                id: id.clone(),
                progress: 100.0,
                speed: None,
                eta: None,
                status: "done".to_string(),
                title: None,
                file_path,
                error_message: None,
                attempt: None,
                max_attempts: None,
            });
        } else {
            let _ = broadcast_tx.send(DownloadProgress {
                id: id.clone(),
                progress: 0.0,
                speed: None,
                eta: None,
                status: "error".to_string(),
                title: None,
                file_path: None,
                error_message: Some(mptube_core::classify_error(&stderr)),
                attempt: None,
                max_attempts: None,
            });
        }
    });

    state
        .handles
        .lock()
        .unwrap()
        .insert(body.id, (handle, ip));

    Ok(StatusCode::ACCEPTED)
}

pub async fn cancel_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> StatusCode {
    if let Some((handle, ip)) = state.handles.lock().unwrap().remove(&id) {
        handle.abort();
        state.decrement_ip(ip);
    }
    let _ = state.progress_tx.send(DownloadProgress {
        id,
        progress: 0.0,
        speed: None,
        eta: None,
        status: "cancelled".to_string(),
        title: None,
        file_path: None,
        error_message: None,
        attempt: None,
        max_attempts: None,
    });
    StatusCode::OK
}

pub async fn file_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let path = state.completed_files.lock().unwrap().get(&id).cloned();
    let Some(path) = path else {
        return Err(err(
            StatusCode::NOT_FOUND,
            "Arquivo não encontrado — pode já ter expirado",
        ));
    };

    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Arquivo não encontrado no disco"))?;

    let filename = sanitize_filename(
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("download"),
    );

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        body,
    )
        .into_response())
}

pub async fn ws_handler(
    State(state): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.progress_tx.subscribe();
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(p) => {
                        let Ok(json) = serde_json::to_string(&p) else { continue };
                        if socket.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

/// Varre `download_dir` periodicamente e remove pastas mais antigas que a retenção configurada.
pub fn spawn_cleanup_task(state: Arc<AppState>) {
    let retention = Duration::from_secs(state.config.retention_minutes * 60);
    let download_dir = state.config.download_dir.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            let Ok(mut entries) = tokio::fs::read_dir(&download_dir).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let Ok(meta) = entry.metadata().await else { continue };
                let Ok(modified) = meta.modified() else { continue };
                if modified.elapsed().map(|e| e > retention).unwrap_or(false) {
                    let _ = tokio::fs::remove_dir_all(entry.path()).await;
                }
            }
        }
    });
}
