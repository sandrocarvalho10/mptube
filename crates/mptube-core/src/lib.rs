//! Lógica de download compartilhada entre o app desktop (Tauri) e o servidor web (Axum).
//! Não depende de tauri nem de axum — só de tokio/serde.

use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc::UnboundedSender;

// ── Progress Event ──────────────────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize)]
pub struct DownloadProgress {
    pub id: String,
    pub progress: f64,
    pub speed: Option<String>,
    pub eta: Option<String>,
    pub status: String,
    pub title: Option<String>,
    pub file_path: Option<String>,
    pub error_message: Option<String>,
    pub attempt: Option<u32>,
    pub max_attempts: Option<u32>,
}

/// Marcador interno usado para reconhecer travamentos nas linhas de stderr coletadas.
const STALL_MARKER: &str = "MPTUBE_STALL_TIMEOUT";

// ── Helpers ──────────────────────────────────────────────────────────────────

pub fn parse_progress(line: &str) -> Option<(f64, Option<String>, Option<String>)> {
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

pub fn build_format_arg(media_type: &str, quality: &str) -> (String, Option<String>) {
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
        "360p" => 360u32,
        "480p" => 480,
        "720p" => 720,
        "1080p" => 1080,
        "1440p" => 1440,
        "2160p" => 2160,
        _ => {
            return (
                "bestvideo+bestaudio/best".to_string(),
                Some(sort_base.to_string()),
            )
        }
    };

    (
        format!(
            "bestvideo[height<={h}]+bestaudio/best[height<={h}]/bestvideo+bestaudio/best",
            h = height
        ),
        Some(format!(
            "res:{h},fps,vcodec:h264,acodec:aac,br,size",
            h = height
        )),
    )
}

pub fn classify_error(stderr_lines: &[String]) -> String {
    if stderr_lines.iter().any(|l| l.contains(STALL_MARKER)) {
        return "Download travado (sem progresso) — tentando novamente".to_string();
    }

    // Filtra só linhas que são erros reais (yt-dlp prefixed com ERROR:)
    let error_lines: Vec<&String> = stderr_lines
        .iter()
        .filter(|l| l.contains("ERROR:") || l.contains("error:"))
        .collect();

    // Usa as linhas de erro se existirem, senão usa tudo para o diagnóstico
    let diagnostic = if !error_lines.is_empty() {
        error_lines
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        stderr_lines.join(" ")
    };

    if diagnostic.contains("HTTP Error 403")
        || diagnostic.contains("returned non-zero exit status") && diagnostic.contains("403")
    {
        return "Acesso negado (403) — importe um arquivo de cookies atualizado".to_string();
    }
    if diagnostic.contains("Sign in to confirm")
        || diagnostic.contains("age-restricted")
        || diagnostic.contains("members-only")
    {
        return "Conteúdo requer login — exporte os cookies após fazer login e importe aqui"
            .to_string();
    }
    if diagnostic.contains("HTTP Error 429") || diagnostic.contains("Too Many Requests") {
        return "Rate limit (429) — aguarde alguns minutos e tente novamente".to_string();
    }
    if diagnostic.contains("Private video") {
        return "Vídeo privado — não é possível baixar".to_string();
    }
    if diagnostic.contains("This video is not available in your country")
        || diagnostic.contains("geo-restricted")
    {
        return "Conteúdo bloqueado na sua região".to_string();
    }
    if diagnostic.contains("This video has been removed")
        || diagnostic.contains("no longer available")
    {
        return "Conteúdo removido ou indisponível".to_string();
    }
    if diagnostic.contains("This live event has ended") {
        return "A transmissão ao vivo já terminou e não está mais disponível".to_string();
    }
    if diagnostic.contains("Video unavailable") {
        return "Vídeo indisponível — verifique se o link ainda é válido".to_string();
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

/// Erros considerados transitórios — vale a pena tentar novamente automaticamente.
fn is_transient_error(stderr_lines: &[String]) -> bool {
    let joined = stderr_lines.join(" ");
    joined.contains(STALL_MARKER)
        || joined.contains("HTTP Error 429")
        || joined.contains("Too Many Requests")
}

/// Constrói o comando yt-dlp.
/// Se `cookie_file` for Some, usa --cookies <arquivo>.
/// Se None, não usa cookies (sem popup, sem browser).
pub fn build_ytdlp_cmd(
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

    // Não forçamos `--extractor-args youtube:player_client=...`: o YouTube muda com
    // frequência quais clients funcionam (ex: rollout "SABR" quebrou ios/web fixos em
    // 2026-09), e o próprio yt-dlp já escolhe e atualiza sua lista de clients padrão
    // a cada release. Forçar um client específico tende a ficar obsoleto mais rápido
    // do que o yt-dlp é atualizado.

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
        .stderr(Stdio::piped())
        // Garante que cancelar o download (abortar a task) mata o processo yt-dlp/ffmpeg
        // de verdade, em vez de deixá-lo órfão rodando em segundo plano.
        .kill_on_drop(true);

    cmd
}

/// Roda um único processo yt-dlp, publicando progresso em `progress_tx`.
/// Mata o processo e retorna erro se ficar `STALL_TIMEOUT` sem nenhuma linha de stdout.
const STALL_TIMEOUT: Duration = Duration::from_secs(120);

pub async fn run_ytdlp(
    mut cmd: Command,
    id: &str,
    progress_tx: &UnboundedSender<DownloadProgress>,
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

    loop {
        match tokio::time::timeout(STALL_TIMEOUT, out_lines.next_line()).await {
            Ok(Ok(Some(line))) => {
                if !line.starts_with('[') && !line.is_empty() {
                    last_file_path = Some(line.clone());
                    continue;
                }
                if let Some((progress, speed, eta)) = parse_progress(&line) {
                    let _ = progress_tx.send(DownloadProgress {
                        id: id.to_string(),
                        progress,
                        speed,
                        eta,
                        status: "downloading".to_string(),
                        title: None,
                        file_path: None,
                        error_message: None,
                        attempt: None,
                        max_attempts: None,
                    });
                }
            }
            Ok(Ok(None)) => break,
            Ok(Err(_)) => break,
            Err(_elapsed) => {
                let _ = child.kill().await;
                return (false, vec![STALL_MARKER.to_string()], None);
            }
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

#[derive(Clone, Copy, Debug)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub base_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_secs(3),
        }
    }
}

/// Roda yt-dlp com retry automático (backoff linear) para erros transitórios
/// (rate limit 429, travamento sem progresso). `build_cmd` reconstrói o `Command`
/// a cada tentativa (um `Command` já usado não pode ser reexecutado).
pub async fn run_ytdlp_with_retry(
    mut build_cmd: impl FnMut() -> Command,
    id: &str,
    progress_tx: &UnboundedSender<DownloadProgress>,
    retry: RetryConfig,
) -> (bool, Vec<String>, Option<String>) {
    let mut attempt: u32 = 1;
    loop {
        let cmd = build_cmd();
        let (success, stderr, file_path) = run_ytdlp(cmd, id, progress_tx).await;

        if success || attempt >= retry.max_attempts || !is_transient_error(&stderr) {
            return (success, stderr, file_path);
        }

        let next_attempt = attempt + 1;
        let _ = progress_tx.send(DownloadProgress {
            id: id.to_string(),
            progress: 0.0,
            speed: None,
            eta: None,
            status: "retrying".to_string(),
            title: None,
            file_path: None,
            error_message: Some(classify_error(&stderr)),
            attempt: Some(next_attempt),
            max_attempts: Some(retry.max_attempts),
        });

        tokio::time::sleep(retry.base_delay * attempt).await;
        attempt = next_attempt;
    }
}

/// Busca os formatos disponíveis de uma URL via `yt-dlp -J` (sem baixar).
/// Retorna título, thumbnail e listas separadas de formatos de vídeo e áudio.
pub async fn fetch_formats_json(
    bin_path: &str,
    ffmpeg_path: Option<&str>,
    cookie_file: Option<&str>,
    url: &str,
) -> Result<serde_json::Value, String> {
    let mut cmd = Command::new(bin_path);

    if let Some(ffmpeg) = ffmpeg_path {
        cmd.arg("--ffmpeg-location").arg(ffmpeg);
    }
    if let Some(cf) = cookie_file {
        cmd.arg("--cookies").arg(cf);
    }

    cmd.arg("--user-agent")
        .arg("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36");
    cmd.arg("--no-playlist");
    cmd.arg("-J") // dump JSON de um único vídeo
        .arg(url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("yt-dlp não encontrado: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(classify_error(
            &stderr.lines().map(String::from).collect::<Vec<_>>(),
        ));
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
        let filesize = f["filesize"].as_u64().or_else(|| f["filesize_approx"].as_u64());
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
                let prev_tbr = video_formats[idx]["tbr"].as_f64().unwrap_or(0.0);
                if tbr > prev_tbr {
                    video_formats[idx] = entry;
                }
            } else {
                seen_res.insert(res_key, video_formats.len());
                video_formats.push(entry);
            }
        } else if has_audio && !has_video {
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

    video_formats.sort_by(|a, b| {
        let ha = a["height"].as_u64().unwrap_or(0);
        let hb = b["height"].as_u64().unwrap_or(0);
        hb.cmp(&ha)
    });

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
