use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ─── State ────────────────────────────────────────────────────────────────────

pub struct AppState {
    /// Handles dos downloads ativos (para cancelamento)
    pub handles: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    /// Caminho do arquivo de cookies importado pelo usuário
    pub cookie_file: Mutex<Option<String>>,
    /// Limita quantos downloads rodam ao mesmo tempo (evita saturar CPU/banda).
    pub download_semaphore: Arc<tokio::sync::Semaphore>,
}

/// Máximo de downloads simultâneos no app desktop.
const MAX_CONCURRENT_DOWNLOADS: usize = 3;

impl Default for AppState {
    fn default() -> Self {
        Self {
            handles: Mutex::new(HashMap::new()),
            cookie_file: Mutex::new(None),
            download_semaphore: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_DOWNLOADS)),
        }
    }
}

// ─── Commands module ─────────────────────────────────────────────────────────

pub mod commands {
    use std::sync::{Arc, Mutex};
    use tauri::{AppHandle, Emitter, Manager, State};
    use tauri_plugin_dialog::DialogExt;

    use mptube_core::{build_format_arg, run_ytdlp_with_retry, DownloadProgress, RetryConfig};

    use super::AppState;

    fn emit_progress(app: &AppHandle, p: DownloadProgress) {
        let _ = app.emit("download-progress", p);
    }

    // ── Sidecar resolution (específico de desktop/Tauri) ─────────────────────

    /// Resolve o caminho de um binário embutido no bundle Tauri (externalBin).
    /// Procura nas localizações conhecidas do Tauri para cada plataforma.
    /// Se não encontrar no bundle, retorna o nome para buscar no PATH (fallback dev).
    fn resolve_sidecar_bin(app: &AppHandle, name: &str) -> String {
        if let Ok(resource_path) = app.path().resource_dir() {
            // macOS: Contents/MacOS/<name>
            let macos_path = resource_path
                .parent()
                .unwrap_or(&resource_path)
                .join("MacOS")
                .join(name);
            if macos_path.exists() {
                return macos_path.to_string_lossy().to_string();
            }

            // Windows: mesma pasta do executável, com extensão .exe
            let win_path = resource_path
                .parent()
                .unwrap_or(&resource_path)
                .join(format!("{}.exe", name));
            if win_path.exists() {
                return win_path.to_string_lossy().to_string();
            }

            // Fallback: diretório de resources direto (alguns builds do Tauri)
            let res_path = resource_path.join(name);
            if res_path.exists() {
                return res_path.to_string_lossy().to_string();
            }
            let res_path_exe = resource_path.join(format!("{}.exe", name));
            if res_path_exe.exists() {
                return res_path_exe.to_string_lossy().to_string();
            }
        }

        // Fallback: busca no PATH do sistema (dev com Homebrew / sistema)
        if cfg!(target_os = "windows") {
            format!("{}.exe", name)
        } else {
            name.to_string()
        }
    }

    /// Resolve o caminho do binário yt-dlp.
    fn resolve_ytdlp_bin(app: &AppHandle) -> String {
        resolve_sidecar_bin(app, "yt-dlp")
    }

    /// Resolve o caminho do binário ffmpeg embutido.
    /// Retorna None se não encontrar no bundle (yt-dlp usará o ffmpeg do PATH).
    fn resolve_ffmpeg_bin(app: &AppHandle) -> Option<String> {
        let candidate = resolve_sidecar_bin(app, "ffmpeg");
        // Se retornou só o nome (sem path absoluto), significa que não encontrou no bundle
        if candidate == "ffmpeg" || candidate == "ffmpeg.exe" {
            None
        } else {
            Some(candidate)
        }
    }

    // ── Commands ──────────────────────────────────────────────────────────────

    /// Busca os formatos disponíveis de uma URL via yt-dlp -J (sem baixar).
    #[tauri::command]
    pub async fn fetch_formats(
        app: AppHandle,
        state: State<'_, Arc<Mutex<AppState>>>,
        url: String,
    ) -> Result<serde_json::Value, String> {
        let bin_path = resolve_ytdlp_bin(&app);
        let ffmpeg_path = resolve_ffmpeg_bin(&app);
        let cookie_file: Option<String> = state
            .lock()
            .unwrap()
            .cookie_file
            .lock()
            .unwrap()
            .clone();

        mptube_core::fetch_formats_json(&bin_path, ffmpeg_path.as_deref(), cookie_file.as_deref(), &url).await
    }

    /// Abre um file picker para o usuário selecionar o arquivo de cookies (.txt)
    /// e salva o caminho no estado global.
    #[tauri::command]
    pub async fn select_cookies_file(
        app: AppHandle,
        state: State<'_, Arc<Mutex<AppState>>>,
    ) -> Result<Option<String>, String> {
        let path = app
            .dialog()
            .file()
            .set_title("Selecionar arquivo de cookies")
            .add_filter("Cookies (Netscape)", &["txt"])
            .blocking_pick_file();

        let result = path.map(|p| p.to_string());
        *state.lock().unwrap().cookie_file.lock().unwrap() = result.clone();
        Ok(result)
    }

    /// Remove o arquivo de cookies do estado.
    #[tauri::command]
    pub fn clear_cookies_file(
        state: State<'_, Arc<Mutex<AppState>>>,
    ) -> Result<(), String> {
        *state.lock().unwrap().cookie_file.lock().unwrap() = None;
        Ok(())
    }

    /// Retorna o caminho atual do arquivo de cookies (para restaurar na UI após reload).
    #[tauri::command]
    pub fn get_cookies_file(
        state: State<'_, Arc<Mutex<AppState>>>,
    ) -> Result<Option<String>, String> {
        Ok(state.lock().unwrap().cookie_file.lock().unwrap().clone())
    }

    #[tauri::command]
    pub async fn start_download(
        app: AppHandle,
        state: State<'_, Arc<Mutex<AppState>>>,
        id: String,
        url: String,
        media_type: String,
        format: String,
        quality: String,
    ) -> Result<(), String> {
        let download_dir = dirs::download_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .to_string_lossy()
            .to_string();

        // quality é sempre uma string de qualidade padrão (ex: "1080p", "320k", "best")
        let (format_arg, format_sort) = build_format_arg(&media_type, &quality);
        let bin_path = resolve_ytdlp_bin(&app);
        let ffmpeg_path = resolve_ffmpeg_bin(&app);
        let semaphore = Arc::clone(&state.lock().unwrap().download_semaphore);

        // Lê o cookie_file do estado (não bloqueia por muito tempo)
        let cookie_file: Option<String> = state
            .lock()
            .unwrap()
            .cookie_file
            .lock()
            .unwrap()
            .clone();

        emit_progress(
            &app,
            DownloadProgress {
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
            },
        );

        let app_clone = app.clone();
        let id_clone = id.clone();
        let state_clone = Arc::clone(&state);

        let handle = tokio::spawn(async move {
            // Encaminha o progresso emitido pelo core (via canal) para o evento Tauri.
            let (progress_tx, mut progress_rx) =
                tokio::sync::mpsc::unbounded_channel::<DownloadProgress>();
            let app_for_forward = app_clone.clone();
            let forwarder = tokio::spawn(async move {
                while let Some(p) = progress_rx.recv().await {
                    let _ = app_for_forward.emit("download-progress", p);
                }
            });

            // Espera um slot livre (no máx. MAX_CONCURRENT_DOWNLOADS downloads ao mesmo tempo).
            let _permit = semaphore.acquire_owned().await;

            let build_cmd = || {
                mptube_core::build_ytdlp_cmd(
                    &bin_path,
                    &url,
                    &media_type,
                    &format,
                    &format_arg,
                    format_sort.as_deref(),
                    &download_dir,
                    cookie_file.as_deref(),
                    ffmpeg_path.as_deref(),
                )
            };

            let (success, stderr, file_path) = run_ytdlp_with_retry(
                build_cmd,
                &id_clone,
                &progress_tx,
                RetryConfig::default(),
            )
            .await;

            drop(progress_tx);
            let _ = forwarder.await;

            state_clone.lock().unwrap().handles.lock().unwrap().remove(&id_clone);

            if success {
                emit_progress(
                    &app_clone,
                    DownloadProgress {
                        id: id_clone,
                        progress: 100.0,
                        speed: None,
                        eta: None,
                        status: "done".to_string(),
                        title: None,
                        file_path,
                        error_message: None,
                        attempt: None,
                        max_attempts: None,
                    },
                );
            } else {
                emit_progress(
                    &app_clone,
                    DownloadProgress {
                        id: id_clone,
                        progress: 0.0,
                        speed: None,
                        eta: None,
                        status: "error".to_string(),
                        title: None,
                        file_path: None,
                        error_message: Some(mptube_core::classify_error(&stderr)),
                        attempt: None,
                        max_attempts: None,
                    },
                );
            }
        });

        state.lock().unwrap().handles.lock().unwrap().insert(id, handle);
        Ok(())
    }

    #[tauri::command]
    pub async fn cancel_download(
        app: AppHandle,
        state: State<'_, Arc<Mutex<AppState>>>,
        id: String,
    ) -> Result<(), String> {
        if let Some(handle) = state.lock().unwrap().handles.lock().unwrap().remove(&id) {
            handle.abort();
        }
        emit_progress(
            &app,
            DownloadProgress {
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
            },
        );
        Ok(())
    }

    #[tauri::command]
    pub fn open_file(path: String) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            // `open -R` revela o arquivo no Finder
            std::process::Command::new("open")
                .arg("-R")
                .arg(&path)
                .spawn()
                .map_err(|e| e.to_string())?;
        }

        #[cfg(target_os = "windows")]
        {
            // /select, revela o arquivo no Explorer
            std::process::Command::new("explorer")
                .arg("/select,")
                .arg(&path)
                .spawn()
                .map_err(|e| e.to_string())?;
        }

        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open")
                .arg(
                    std::path::Path::new(&path)
                        .parent()
                        .unwrap_or(std::path::Path::new(&path)),
                )
                .spawn()
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    }
}

// ─── App Entry ────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(Mutex::new(AppState::default())))
        .invoke_handler(tauri::generate_handler![
            commands::fetch_formats,
            commands::start_download,
            commands::cancel_download,
            commands::open_file,
            commands::select_cookies_file,
            commands::clear_cookies_file,
            commands::get_cookies_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
