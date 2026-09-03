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

    fn build_format_arg(media_type: &str, quality: &str) -> (String, Option<String>) {
        if media_type == "audio" {
            return (
                "bestaudio/best".to_string(),
                Some("abr,asr,br,size".to_string()),
            );
        }

        // Ordena preferindo H.264 (avc1) + AAC (mp4a) para compatibilidade
        // máxima com QuickTime, iPhone, etc.
        // VP9/opus são preferidos pelo YouTube mas não rodam no QuickTime.
        let sort_base = "res,fps,vcodec:h264,acodec:aac,br,size";

        let height = match quality {
            "360p"  => 360u32,
            "480p"  => 480,
            "720p"  => 720,
            "1080p" => 1080,
            "1440p" => 1440,
            "2160p" => 2160,
            _ => return (
                "bestvideo+bestaudio/best".to_string(),
                Some(sort_base.to_string()),
            ),
        };

        (
            format!(
                "bestvideo[height<={h}]+bestaudio/best[height<={h}]/bestvideo+bestaudio/best",
                h = height
            ),
            Some(format!("res:{h},fps,vcodec:h264,acodec:aac,br,size", h = height)),
        )
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
            return "ffmpeg não encontrado — reinstale o mptube para restaurar os binários".to_string();
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

    /// Constrói o comando yt-dlp.
    /// Se `cookie_file` for Some, usa --cookies <arquivo>.
    /// Se None, não usa cookies (sem popup, sem browser).
    fn build_ytdlp_cmd(
        bin_path: &str,
        url: &str,
        media_type: &str,
        format: &str,
        format_arg: &str,
        format_sort: Option<&str>,
        download_dir: &str,
        cookie_file: Option<&str>,
        ffmpeg_path: Option<&str>,
    ) -> Command {
        let mut cmd = Command::new(bin_path);

        // ── ffmpeg bundled ────────────────────────────────────────────────────
        // Se o ffmpeg está embutido no bundle, apontamos diretamente para ele.
        // Isso garante que no Windows não seja necessário instalar o ffmpeg separado.
        if let Some(ffmpeg) = ffmpeg_path {
            cmd.arg("--ffmpeg-location").arg(ffmpeg);
        }

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

        // ── YouTube: player client ────────────────────────────────────────────
        // "ios" entrega streams adaptativos até 4K sem cookies.
        // "web" como fallback para conteúdos que o iOS não suporta.
        // Evitamos "android" pois limita os streams a 360p no formato adaptativo.
        cmd.arg("--extractor-args")
            .arg("youtube:player_client=ios,web");

        // ── Output ────────────────────────────────────────────────────────────
        cmd.arg("--newline")
            .arg("--progress")
            .arg("-f")
            .arg(format_arg);

        if let Some(sort) = format_sort {
            cmd.arg("--format-sort").arg(sort);
        }

        cmd.arg("-o")
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
            // Força H.264 + AAC em MP4 para compatibilidade com QuickTime/iPhone.
            // -c:v libx264: reencoda vídeo para H.264 (crf 18 = alta qualidade)
            // -c:a aac -b:a 192k: reencoda áudio para AAC (cobre HE-AAC, opus, vorbis)
            // -movflags +faststart: otimiza MP4 para streaming/preview rápido
            cmd.arg("--merge-output-format").arg("mp4")
               .arg("--postprocessor-args")
               .arg("Merger+ffmpeg:-c:v libx264 -crf 18 -preset fast -c:a aac -b:a 192k -movflags +faststart");
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

    /// Busca os formatos disponíveis de uma URL via yt-dlp -J (sem baixar).
    /// Retorna título, thumbnail e listas separadas de formatos de vídeo e áudio.
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

        let mut cmd = Command::new(&bin_path);

        if let Some(ffmpeg) = &ffmpeg_path {
            cmd.arg("--ffmpeg-location").arg(ffmpeg);
        }
        if let Some(cf) = &cookie_file {
            cmd.arg("--cookies").arg(cf);
        }

        cmd.arg("--user-agent")
            .arg("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36");
        cmd.arg("--extractor-args")
            .arg("youtube:player_client=ios,web");
        cmd.arg("--no-playlist");
        cmd.arg("-J")   // dump JSON de um único vídeo
            .arg(&url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = cmd.output().await.map_err(|e| format!("yt-dlp não encontrado: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(classify_error(&stderr.lines().map(String::from).collect::<Vec<_>>()));
        }

        let raw: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("Erro ao parsear formatos: {}", e))?;

        let title = raw["title"].as_str().unwrap_or("").to_string();
        let thumbnail = raw["thumbnail"].as_str().map(String::from);
        let duration = raw["duration"].as_f64();
        let webpage_url = raw["webpage_url"].as_str().map(String::from);

        let empty = vec![];
        let formats = raw["formats"].as_array().unwrap_or(&empty);

        let mut video_formats: Vec<serde_json::Value> = Vec::new();
        let mut audio_formats: Vec<serde_json::Value> = Vec::new();
        // Deduplica formatos de vídeo por resolução (mantém o de maior tbr)
        let mut seen_res: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

        for f in formats {
            let vcodec = f["vcodec"].as_str().unwrap_or("none");
            let acodec = f["acodec"].as_str().unwrap_or("none");
            let has_video = vcodec != "none" && !vcodec.is_empty();
            let has_audio = acodec != "none" && !acodec.is_empty();

            let ext = f["ext"].as_str().unwrap_or("").to_string();
            let format_id = f["format_id"].as_str().unwrap_or("").to_string();
            let filesize = f["filesize"].as_u64()
                .or_else(|| f["filesize_approx"].as_u64());
            let tbr = f["tbr"].as_f64().unwrap_or(0.0);
            let abr = f["abr"].as_f64().unwrap_or(0.0);
            let vbr = f["vbr"].as_f64().unwrap_or(0.0);
            let fps = f["fps"].as_f64();
            let width = f["width"].as_u64();
            let height = f["height"].as_u64();
            let asr = f["asr"].as_u64(); // sample rate

            if has_video {
                let res_key = format!("{}x{}", width.unwrap_or(0), height.unwrap_or(0));
                let entry = serde_json::json!({
                    "format_id": format_id,
                    "ext": ext,
                    "width": width,
                    "height": height,
                    "fps": fps,
                    "vcodec": vcodec,
                    "acodec": if has_audio { acodec } else { "none" },
                    "tbr": tbr,
                    "vbr": vbr,
                    "filesize": filesize,
                    "has_audio": has_audio,
                    "label": if let (Some(h), Some(fps_v)) = (height, fps) {
                        format!("{}p {:.0}fps", h, fps_v)
                    } else if let Some(h) = height {
                        format!("{}p", h)
                    } else {
                        format!("{} ({})", format_id, ext.to_uppercase())
                    },
                });

                if let Some(&idx) = seen_res.get(&res_key) {
                    // Substitui se tbr maior (melhor qualidade na mesma resolução)
                    let prev_tbr = video_formats[idx]["tbr"].as_f64().unwrap_or(0.0);
                    if tbr > prev_tbr {
                        video_formats[idx] = entry;
                    }
                } else {
                    seen_res.insert(res_key, video_formats.len());
                    video_formats.push(entry);
                }
            } else if has_audio && !has_video {
                // Formatos de áudio puro
                let abr_val = if abr > 0.0 { abr } else { tbr };
                audio_formats.push(serde_json::json!({
                    "format_id": format_id,
                    "ext": ext,
                    "acodec": acodec,
                    "abr": abr_val,
                    "asr": asr,
                    "filesize": filesize,
                    "label": if abr_val > 0.0 {
                        format!("{:.0} kbps ({})", abr_val, ext.to_uppercase())
                    } else {
                        format!("{} ({})", format_id, ext.to_uppercase())
                    },
                }));
            }
        }

        // Ordena vídeos por altura decrescente
        video_formats.sort_by(|a, b| {
            let ha = a["height"].as_u64().unwrap_or(0);
            let hb = b["height"].as_u64().unwrap_or(0);
            hb.cmp(&ha)
        });

        // Ordena áudios por bitrate decrescente
        audio_formats.sort_by(|a, b| {
            let ba = a["abr"].as_f64().unwrap_or(0.0);
            let bb = b["abr"].as_f64().unwrap_or(0.0);
            bb.partial_cmp(&ba).unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(serde_json::json!({
            "title": title,
            "thumbnail": thumbnail,
            "duration": duration,
            "webpage_url": webpage_url,
            "video_formats": video_formats,
            "audio_formats": audio_formats,
        }))
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
                format_sort.as_deref(),
                &download_dir,
                cookie_file.as_deref(),
                ffmpeg_path.as_deref(),
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
