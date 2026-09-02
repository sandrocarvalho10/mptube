use std::collections::HashMap;
use std::sync::Mutex;

// ─── State ────────────────────────────────────────────────────────────────────

pub struct AppState {
    /// Handles dos downloads ativos (para cancelamento)
    pub handles: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    /// Caminho do arquivo de cookies importado pelo usuário
    pub cookie_file: Mutex<Option<String>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            handles: Mutex::new(HashMap::new()),
            cookie_file: Mutex::new(None),
        }
    }
}

// ─── Commands module ─────────────────────────────────────────────────────────

pub mod commands {
    use std::process::Stdio;
    use std::sync::{Arc, Mutex};
    use tauri::{AppHandle, Emitter, Manager, State};
    use tauri_plugin_dialog::DialogExt;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;

    use super::AppState;

    // ── Progress Event ────────────────────────────────────────────────────────

    #[derive(Clone, serde::Serialize)]
    pub struct DownloadProgress {
        pub id: String,
        pub progress: f64,
        pub speed: Option<String>,
        pub eta: Option<String>,
        pub status: String,
        pub title: Option<String>,
        pub file_path: Option<String>,
        pub error_message: Option<String>,
    }

    fn emit_progress(app: &AppHandle, p: DownloadProgress) {
        let _ = app.emit("download-progress", p);
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn parse_progress(line: &str) -> Option<(f64, Option<String>, Option<String>)> {
        if !line.contains("[download]") {
            return None;
        }
        let mut progress: Option<f64> = None;
        let mut speed: Option<String> = None;
        let mut eta: Option<String> = None;
        let has_eta = line.contains("ETA");

        for part in line.split_whitespace() {
            if part.ends_with('%') {
                progress = part.trim_end_matches('%').parse().ok();
            } else if part.ends_with("/s") {
                speed = Some(part.to_string());
            } else if has_eta && part.contains(':') && eta.is_none() {
                eta = Some(part.to_string());
            }
        }
        progress.map(|p| (p, speed, eta))
    }

    fn build_format_arg(media_type: &str, quality: &str) -> String {
        if media_type == "audio" {
            return "bestaudio[ext=m4a]/bestaudio/best".to_string();
        }
        let height = match quality {
            "360p"  => "360",
            "480p"  => "480",
            "720p"  => "720",
            "1080p" => "1080",
            "1440p" => "1440",
            "2160p" => "2160",
            _       => return "bestvideo+bestaudio/best".to_string(),
        };
        format!("bestvideo[height<={h}]+bestaudio/best[height<={h}]", h = height)
    }

    fn classify_error(stderr_lines: &[String]) -> String {
        // Filtra só linhas que são erros reais (yt-dlp prefixed com ERROR:)
        let error_lines: Vec<&String> = stderr_lines
            .iter()
            .filter(|l| l.contains("ERROR:") || l.contains("error:"))
            .collect();

        // Usa as linhas de erro se existirem, senão usa tudo para o diagnóstico
        let diagnostic = if !error_lines.is_empty() {
            error_lines.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ")
        } else {
            stderr_lines.join(" ")
        };

        if diagnostic.contains("HTTP Error 403") || diagnostic.contains("returned non-zero exit status") && diagnostic.contains("403") {
            return "Acesso negado (403) — importe um arquivo de cookies atualizado".to_string();
        }
        if diagnostic.contains("Sign in to confirm") || diagnostic.contains("age-restricted") || diagnostic.contains("members-only") {
            return "Conteúdo requer login — exporte os cookies após fazer login e importe aqui".to_string();
        }
        if diagnostic.contains("HTTP Error 429") || diagnostic.contains("Too Many Requests") {
            return "Rate limit (429) — aguarde alguns minutos e tente novamente".to_string();
        }
        if diagnostic.contains("Private video") {
            return "Vídeo privado — não é possível baixar".to_string();
        }
        if diagnostic.contains("This video is not available in your country") || diagnostic.contains("geo-restricted") {
            return "Conteúdo bloqueado na sua região".to_string();
        }
        if diagnostic.contains("This video has been removed") || diagnostic.contains("no longer available") {
            return "Conteúdo removido ou indisponível".to_string();
        }
        if diagnostic.contains("Unsupported URL") || diagnostic.contains("Unable to extract") {
            return "URL não suportada — verifique o link".to_string();
        }
        if diagnostic.contains("ffmpeg") && (diagnostic.contains("not found") || diagnostic.contains("No such file")) {
            return "ffmpeg não encontrado — instale via 'brew install ffmpeg' (Mac) ou https://ffmpeg.org (Windows)".to_string();
        }
        if diagnostic.contains("Unable to download") && diagnostic.contains("Cookies") {
            return "Arquivo de cookies inválido ou expirado — exporte novamente".to_string();
        }

        // Fallback: retorna a primeira linha de erro real, sem reinterpretar
        error_lines
            .first()
            .map(|l| {
                // Remove o prefixo "ERROR: [youtube] id: " para ficar mais limpo
                let msg = l.trim();
                if let Some(pos) = msg.find("]: ") {
                    msg[pos + 3..].to_string()
                } else {
                    msg.to_string()
                }
            })
            .unwrap_or_else(|| "Download falhou — verifique a URL e tente novamente".to_string())
    }

    /// Resolve o caminho do binário yt-dlp.
    /// 1. Tenta o binário embutido no bundle Tauri (externalBin)
    /// 2. Fallback para o yt-dlp do PATH do sistema (dev / Homebrew)
    fn resolve_ytdlp_bin(app: &AppHandle) -> String {
        // tauri::api::process::current_binary retorna o executável do app.
        // O Tauri coloca os externalBin na mesma pasta com sufixo de target triple.
        if let Ok(resource_path) = app.path().resource_dir() {
            // No bundle, os sidecar ficam em:
            //   macOS: Contents/MacOS/yt-dlp
            //   Windows: yt-dlp.exe  (na pasta do .exe)
            let sidecar = resource_path
                .parent()                          // sai de Resources → Contents
                .unwrap_or(&resource_path)
                .join("MacOS")                     // macOS bundle path
                .join("yt-dlp");

            if sidecar.exists() {
                return sidecar.to_string_lossy().to_string();
            }

            // Windows: mesmo diretório do executável
            let sidecar_win = resource_path
                .parent()
                .unwrap_or(&resource_path)
                .join("yt-dlp.exe");

            if sidecar_win.exists() {
                return sidecar_win.to_string_lossy().to_string();
            }
        }

        // Fallback: usa o yt-dlp do PATH (funciona em dev com Homebrew)
        if cfg!(target_os = "windows") {
            "yt-dlp.exe".to_string()
        } else {
            "yt-dlp".to_string()
        }
    }

    /// Constrói o comando yt-dlp.
    /// Se `cookie_file` for Some, usa --cookies <arquivo>.
    /// Se None, não usa cookies (sem popup, sem browser).
    fn build_ytdlp_cmd(
        bin_path: &str,
        url: &str,
        media_type: &str,
        format: &str,
        format_arg: &str,
        download_dir: &str,
        cookie_file: Option<&str>,
    ) -> Command {
        let mut cmd = Command::new(bin_path);

        // ── Cookies ───────────────────────────────────────────────────────────
        if let Some(path) = cookie_file {
            cmd.arg("--cookies").arg(path);
        }

        // ── User-Agent / Headers ──────────────────────────────────────────────
        // Nota: --impersonate requer curl_cffi que pode não estar instalado.
        // Usamos user-agent e headers diretos, que funcionam sem dependências extras.
        cmd.arg("--user-agent")
            .arg("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36");
        cmd.arg("--add-header")
            .arg("Accept-Language:pt-BR,pt;q=0.9,en-US;q=0.8,en;q=0.7");
        cmd.arg("--add-header")
            .arg("Accept:text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8");

        // ── Retries ───────────────────────────────────────────────────────────
        cmd.arg("--retries").arg("5");
        cmd.arg("--fragment-retries").arg("5");

        // ── Geo-bypass ────────────────────────────────────────────────────────
        cmd.arg("--geo-bypass");

        // ── YouTube: player client android evita bot-check ───────────────────
        cmd.arg("--extractor-args")
            .arg("youtube:player_client=android,web;player_skip=webpage");

        // ── Output ────────────────────────────────────────────────────────────
        cmd.arg("--newline")
            .arg("--progress")
            .arg("-f")
            .arg(format_arg)
            .arg("-o")
            .arg(format!("{}/%(title)s.%(ext)s", download_dir))
            .arg("--print")
            .arg("after_move:filepath");

        if media_type == "audio" {
            cmd.arg("-x")
                .arg("--audio-format")
                .arg(format)
                .arg("--audio-quality")
                .arg("0");
        } else {
            cmd.arg("--merge-output-format").arg(format);
        }

        cmd.arg(url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        cmd
    }

    async fn run_ytdlp(
        mut cmd: Command,
        id: &str,
        app: &AppHandle,
    ) -> (bool, Vec<String>, Option<String>) {
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return (false, vec![format!("yt-dlp não encontrado: {}", e)], None);
            }
        };

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let mut out_lines = BufReader::new(stdout).lines();
        let mut err_lines = BufReader::new(stderr).lines();
        let mut last_file_path: Option<String> = None;

        while let Ok(Some(line)) = out_lines.next_line().await {
            if !line.starts_with('[') && !line.is_empty() {
                last_file_path = Some(line.clone());
                continue;
            }
            if let Some((progress, speed, eta)) = parse_progress(&line) {
                emit_progress(
                    app,
                    DownloadProgress {
                        id: id.to_string(),
                        progress,
                        speed,
                        eta,
                        status: "downloading".to_string(),
                        title: None,
                        file_path: None,
                        error_message: None,
                    },
                );
            }
        }

        let mut stderr_buf = Vec::new();
        while let Ok(Some(line)) = err_lines.next_line().await {
            if !line.is_empty() {
                stderr_buf.push(line);
            }
        }

        let success = child.wait().await.map(|s| s.success()).unwrap_or(false);
        (success, stderr_buf, last_file_path)
    }

    // ── Commands ──────────────────────────────────────────────────────────────

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

        let format_arg = build_format_arg(&media_type, &quality);
        let bin_path = resolve_ytdlp_bin(&app);

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
            },
        );

        let app_clone = app.clone();
        let id_clone = id.clone();
        let state_clone = Arc::clone(&state);

        let handle = tokio::spawn(async move {
            let cmd = build_ytdlp_cmd(
                &bin_path,
                &url,
                &media_type,
                &format,
                &format_arg,
                &download_dir,
                cookie_file.as_deref(),
            );

            let (success, stderr, file_path) = run_ytdlp(cmd, &id_clone, &app_clone).await;

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
                        error_message: Some(classify_error(&stderr)),
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
    use std::sync::{Arc, Mutex};
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(Mutex::new(AppState::default())))
        .invoke_handler(tauri::generate_handler![
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
