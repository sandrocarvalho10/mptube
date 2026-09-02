import { useState, useEffect } from "react";
import {
  ArrowDownToLine,
  Clipboard,
  X,
  Clapperboard,
  Music,
  ChevronDown,
  Loader2,
} from "lucide-react";
import {
  MediaType,
  MediaFormat,
  MediaQuality,
  AudioFormat,
  VideoFormat,
  AudioQuality,
  VideoQuality,
  DownloadRequest,
  detectSource,
  SOURCE_LABELS,
  VIDEO_QUALITY_LABELS,
  AUDIO_QUALITY_LABELS,
} from "../types";

interface Props {
  onDownload: (req: DownloadRequest) => void;
  isLoading?: boolean;
}

const AUDIO_FORMATS: AudioFormat[] = ["mp3", "aac", "opus", "flac", "wav"];
const VIDEO_FORMATS: VideoFormat[] = ["mp4", "mkv", "mov", "webm"];
const AUDIO_QUALITIES: AudioQuality[] = ["128k", "192k", "256k", "320k"];
const VIDEO_QUALITIES: VideoQuality[] = ["360p", "480p", "720p", "1080p", "1440p", "2160p", "best"];

export function DownloaderForm({ onDownload, isLoading = false }: Props) {
  const [url, setUrl] = useState("");
  const [mediaType, setMediaType] = useState<MediaType>("video");
  const [format, setFormat] = useState<MediaFormat>("mp4");
  const [quality, setQuality] = useState<MediaQuality>("1080p");
  const [detectedSource, setDetectedSource] = useState<string | null>(null);
  const [urlError, setUrlError] = useState<string | null>(null);

  useEffect(() => {
    if (!url.trim()) {
      setDetectedSource(null);
      setUrlError(null);
      return;
    }
    try {
      new URL(url.trim());
      const src = detectSource(url);
      setDetectedSource(SOURCE_LABELS[src]);
      setUrlError(null);
    } catch {
      setDetectedSource(null);
      setUrlError("URL inválida");
    }
  }, [url]);

  useEffect(() => {
    if (mediaType === "audio") {
      setFormat("mp3");
      setQuality("320k");
    } else {
      setFormat("mp4");
      setQuality("1080p");
    }
  }, [mediaType]);

  const formats = mediaType === "audio" ? AUDIO_FORMATS : VIDEO_FORMATS;
  const qualities =
    mediaType === "audio"
      ? AUDIO_QUALITIES.map((q) => ({ value: q, label: AUDIO_QUALITY_LABELS[q] }))
      : VIDEO_QUALITIES.map((q) => ({ value: q, label: VIDEO_QUALITY_LABELS[q] }));

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!url.trim() || urlError) return;
    onDownload({ url: url.trim(), mediaType, format, quality });
    setUrl("");
  }

  function handlePaste() {
    navigator.clipboard.readText().then((text) => {
      if (text) setUrl(text.trim());
    });
  }

  const canSubmit = url.trim().length > 0 && !urlError && !isLoading;

  return (
    <form className="downloader-form" onSubmit={handleSubmit}>
      {/* Header */}
      <div className="form-header">
        <span className="form-icon">
          <ArrowDownToLine size={26} strokeWidth={2.5} />
        </span>
        <div>
          <h1 className="form-title">mptube</h1>
          <p className="form-subtitle">Baixe vídeos e áudios de qualquer fonte</p>
        </div>
      </div>

      {/* URL Input */}
      <div className="form-group">
        <label className="form-label">Cole uma URL</label>
        <div className={`url-input-wrapper ${urlError ? "has-error" : ""} ${detectedSource ? "has-source" : ""}`}>
          {detectedSource && (
            <span className="source-badge">{detectedSource}</span>
          )}
          <input
            type="text"
            className="url-input"
            placeholder="https://youtube.com/watch?v=..."
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            spellCheck={false}
            autoComplete="off"
          />
          <div className="url-actions">
            {url && (
              <button
                type="button"
                className="btn-icon"
                title="Limpar"
                onClick={() => setUrl("")}
              >
                <X size={14} strokeWidth={2.5} />
              </button>
            )}
            <button
              type="button"
              className="btn-icon btn-paste"
              title="Colar da área de transferência"
              onClick={handlePaste}
            >
              <Clipboard size={14} strokeWidth={2} />
            </button>
          </div>
        </div>
        {urlError && <span className="field-error">{urlError}</span>}
      </div>

      {/* Media Type */}
      <div className="form-group">
        <label className="form-label">Tipo</label>
        <div className="radio-group">
          {(["video", "audio"] as MediaType[]).map((t) => (
            <label key={t} className={`radio-option ${mediaType === t ? "active" : ""}`}>
              <input
                type="radio"
                name="mediaType"
                value={t}
                checked={mediaType === t}
                onChange={() => setMediaType(t)}
              />
              <span className="radio-icon">
                {t === "video"
                  ? <Clapperboard size={16} strokeWidth={1.8} />
                  : <Music size={16} strokeWidth={1.8} />
                }
              </span>
              <span className="radio-label">{t === "video" ? "Vídeo" : "Áudio"}</span>
            </label>
          ))}
        </div>
      </div>

      {/* Format & Quality Row */}
      <div className="form-row">
        <div className="form-group">
          <label className="form-label">Formato</label>
          <div className="select-wrapper">
            <select
              className="form-select"
              value={format}
              onChange={(e) => setFormat(e.target.value as MediaFormat)}
            >
              {formats.map((f) => (
                <option key={f} value={f}>
                  {f.toUpperCase()}
                </option>
              ))}
            </select>
            <ChevronDown size={14} className="select-chevron" strokeWidth={2} />
          </div>
        </div>

        <div className="form-group">
          <label className="form-label">Qualidade</label>
          <div className="select-wrapper">
            <select
              className="form-select"
              value={quality}
              onChange={(e) => setQuality(e.target.value as MediaQuality)}
            >
              {qualities.map((q) => (
                <option key={q.value} value={q.value}>
                  {q.label}
                </option>
              ))}
            </select>
            <ChevronDown size={14} className="select-chevron" strokeWidth={2} />
          </div>
        </div>
      </div>

      {/* Submit */}
      <button
        type="submit"
        className={`btn-download ${isLoading ? "loading" : ""}`}
        disabled={!canSubmit}
      >
        {isLoading ? (
          <>
            <Loader2 size={16} strokeWidth={2.5} className="spin-icon" />
            Processando...
          </>
        ) : (
          <>
            <ArrowDownToLine size={16} strokeWidth={2.5} />
            BAIXAR
          </>
        )}
      </button>
    </form>
  );
}
