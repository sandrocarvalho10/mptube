// ─── Media Types ────────────────────────────────────────────────────────────

export type MediaType = "video" | "audio" | "image" | "reel";

// ─── Format Options ─────────────────────────────────────────────────────────

export type AudioFormat = "mp3" | "aac" | "opus" | "flac" | "wav";
export type VideoFormat = "mp4" | "mkv" | "mov" | "webm" | "avi";
export type ImageFormat = "jpg" | "png" | "webp";
export type MediaFormat = AudioFormat | VideoFormat | ImageFormat;

// ─── Quality Options ─────────────────────────────────────────────────────────

export type AudioQuality = "128k" | "192k" | "256k" | "320k";
export type VideoQuality = "360p" | "480p" | "720p" | "1080p" | "1440p" | "2160p" | "best";

export type MediaQuality = AudioQuality | VideoQuality;

export const VIDEO_QUALITY_LABELS: Record<VideoQuality, string> = {
  "360p": "360p",
  "480p": "480p",
  "720p": "HD 720p",
  "1080p": "Full HD 1080p",
  "1440p": "2K 1440p",
  "2160p": "4K 2160p",
  "best": "Melhor disponível",
};

export const AUDIO_QUALITY_LABELS: Record<AudioQuality, string> = {
  "128k": "128 kbps",
  "192k": "192 kbps",
  "256k": "256 kbps",
  "320k": "320 kbps (máx)",
};

// ─── Download Status ─────────────────────────────────────────────────────────

export type DownloadStatus =
  | "queued"
  | "fetching_info"
  | "downloading"
  | "converting"
  | "retrying"
  | "done"
  | "error"
  | "cancelled";

// ─── Download Item ───────────────────────────────────────────────────────────

export interface DownloadItem {
  id: string;
  url: string;
  title: string;
  thumbnail?: string;
  mediaType: MediaType;
  format: MediaFormat;
  /** Qualidade legada (ex: "1080p") ou format_id real do yt-dlp (ex: "137") */
  quality: MediaQuality | string;
  status: DownloadStatus;
  progress: number;        // 0–100
  speed?: string;          // e.g. "2.3 MiB/s"
  eta?: string;            // e.g. "00:45"
  filePath?: string;
  errorMessage?: string;
  /** Preenchido quando o backend está tentando novamente após um erro transitório. */
  attempt?: number;
  maxAttempts?: number;
  createdAt: number;       // timestamp ms
}

// ─── Download Request ────────────────────────────────────────────────────────

export interface DownloadRequest {
  url: string;
  mediaType: MediaType;
  format: MediaFormat;
  /** Qualidade legada (ex: "1080p") ou format_id real do yt-dlp (ex: "137") */
  quality: MediaQuality | string;
  outputDir?: string;
  /** Conhecidos desde a etapa de seleção de formato — usados só para exibição imediata. */
  title?: string;
  thumbnail?: string;
}

// ─── Source Detection ────────────────────────────────────────────────────────

export type MediaSource =
  | "youtube"
  | "instagram"
  | "tiktok"
  | "twitter"
  | "facebook"
  | "vimeo"
  | "soundcloud"
  | "generic";

export function detectSource(url: string): MediaSource {
  if (/youtube\.com|youtu\.be/.test(url)) return "youtube";
  if (/instagram\.com/.test(url)) return "instagram";
  if (/tiktok\.com/.test(url)) return "tiktok";
  if (/twitter\.com|x\.com/.test(url)) return "twitter";
  if (/facebook\.com|fb\.watch/.test(url)) return "facebook";
  if (/vimeo\.com/.test(url)) return "vimeo";
  if (/soundcloud\.com/.test(url)) return "soundcloud";
  return "generic";
}

export const SOURCE_LABELS: Record<MediaSource, string> = {
  youtube: "YouTube",
  instagram: "Instagram",
  tiktok: "TikTok",
  twitter: "Twitter / X",
  facebook: "Facebook",
  vimeo: "Vimeo",
  soundcloud: "SoundCloud",
  generic: "Link",
};

// ─── Tauri Event Payloads ────────────────────────────────────────────────────

export interface DownloadProgress {
  id: string;
  progress: number;
  speed?: string;
  eta?: string;
  status: DownloadStatus;
  title?: string;
  filePath?: string;
  errorMessage?: string;
  attempt?: number;
  maxAttempts?: number;
}

// ─── Format Picker ───────────────────────────────────────────────────────────

export interface VideoFormatInfo {
  format_id: string;
  ext: string;
  width?: number;
  height?: number;
  fps?: number;
  vcodec: string;
  acodec: string;   // "none" se stream adaptativo (sem áudio embutido)
  tbr: number;
  vbr: number;
  filesize?: number;
  has_audio: boolean;
  label: string;    // ex: "1080p 30fps"
}

export interface AudioFormatInfo {
  format_id: string;
  ext: string;
  acodec: string;
  abr: number;
  asr?: number;
  filesize?: number;
  label: string;    // ex: "128 kbps (M4A)"
}

export interface FetchFormatsResult {
  title: string;
  thumbnail?: string;
  duration?: number;
  webpage_url?: string;
  video_formats: VideoFormatInfo[];
  audio_formats: AudioFormatInfo[];
}
