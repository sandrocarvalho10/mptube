import { useState } from "react";
import { Clapperboard, Music, Download, HardDrive, Zap, ArrowLeft } from "lucide-react";
import { VideoFormatInfo, AudioFormatInfo, FetchFormatsResult } from "../types";

interface Props {
  result: FetchFormatsResult;
  onSelect: (quality: string, mediaType: "video" | "audio", ext: string) => void;
  onBack: () => void;
  isLoading: boolean;
}

function formatBytes(bytes?: number): string {
  if (!bytes) return "";
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function formatDuration(secs?: number): string {
  if (!secs) return "";
  const m = Math.floor(secs / 60);
  const s = Math.floor(secs % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}

export function FormatPicker({ result, onSelect, onBack, isLoading }: Props) {
  const [tab, setTab] = useState<"video" | "audio">("video");
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const { title, thumbnail, duration, video_formats, audio_formats } = result;

  function handleDownload() {
    if (!selectedId) return;
    if (tab === "video") {
      const fmt = video_formats.find((f) => f.format_id === selectedId);
      if (!fmt) return;
      // Passa a altura como qualidade — o backend constrói o seletor correto
      const q = fmt.height ? `${fmt.height}p` : "best";
      onSelect(q, "video", fmt.ext);
    } else {
      const fmt = audio_formats.find((f) => f.format_id === selectedId);
      if (!fmt) return;
      // Para áudio passa o bitrate arredondado (ex: "128k", "320k") ou "best"
      const abr = fmt.abr ?? 0;
      const q = abr >= 256 ? "320k" : abr >= 192 ? "256k" : abr >= 128 ? "192k" : abr > 0 ? "128k" : "best";
      onSelect(q, "audio", fmt.ext);
    }
  }

  // Deselect quando troca de aba
  function switchTab(t: "video" | "audio") {
    setTab(t);
    setSelectedId(null);
  }

  const currentFormats = tab === "video" ? video_formats : audio_formats;
  const hasSelection = selectedId !== null;

  return (
    <div className="format-picker">
      {/* Thumbnail + info */}
      <div className="fp-meta">
        {thumbnail && (
          <img src={thumbnail} alt={title} className="fp-thumbnail" />
        )}
        <div className="fp-meta-info">
          <p className="fp-title">{title}</p>
          {duration && (
            <span className="fp-duration">{formatDuration(duration)}</span>
          )}
        </div>
      </div>

      {/* Tabs */}
      <div className="fp-tabs">
        <button
          className={`fp-tab ${tab === "video" ? "active" : ""}`}
          onClick={() => switchTab("video")}
          type="button"
        >
          <Clapperboard size={14} strokeWidth={1.8} />
          Vídeo
          <span className="fp-tab-count">{video_formats.length}</span>
        </button>
        <button
          className={`fp-tab ${tab === "audio" ? "active" : ""}`}
          onClick={() => switchTab("audio")}
          type="button"
        >
          <Music size={14} strokeWidth={1.8} />
          Áudio
          <span className="fp-tab-count">{audio_formats.length}</span>
        </button>
      </div>

      {/* Format list */}
      <div className="fp-list">
        {currentFormats.length === 0 && (
          <p className="fp-empty">Nenhum formato disponível nesta categoria.</p>
        )}

        {tab === "video" &&
          (video_formats as VideoFormatInfo[]).map((f) => (
            <button
              key={f.format_id}
              type="button"
              className={`fp-card ${selectedId === f.format_id ? "selected" : ""}`}
              onClick={() => setSelectedId(f.format_id)}
            >
              <div className="fp-card-main">
                <span className="fp-card-label">{f.label}</span>
                <div className="fp-card-badges">
                  <span className="fp-badge fp-badge--codec">{f.vcodec.split(".")[0]}</span>
                  <span className="fp-badge fp-badge--ext">{f.ext.toUpperCase()}</span>
                  {f.has_audio && (
                    <span className="fp-badge fp-badge--audio" title="Inclui áudio">
                      <Music size={9} strokeWidth={2} /> áudio
                    </span>
                  )}
                </div>
              </div>
              <div className="fp-card-right">
                {f.tbr > 0 && (
                  <span className="fp-card-meta">
                    <Zap size={10} strokeWidth={2} />
                    {f.tbr.toFixed(0)} kbps
                  </span>
                )}
                {f.filesize && (
                  <span className="fp-card-meta">
                    <HardDrive size={10} strokeWidth={2} />
                    {formatBytes(f.filesize)}
                  </span>
                )}
              </div>
            </button>
          ))}

        {tab === "audio" &&
          (audio_formats as AudioFormatInfo[]).map((f) => (
            <button
              key={f.format_id}
              type="button"
              className={`fp-card ${selectedId === f.format_id ? "selected" : ""}`}
              onClick={() => setSelectedId(f.format_id)}
            >
              <div className="fp-card-main">
                <span className="fp-card-label">{f.label}</span>
                <div className="fp-card-badges">
                  <span className="fp-badge fp-badge--codec">{f.acodec.split(".")[0]}</span>
                  {f.asr && (
                    <span className="fp-badge">{(f.asr / 1000).toFixed(0)} kHz</span>
                  )}
                </div>
              </div>
              <div className="fp-card-right">
                {f.filesize && (
                  <span className="fp-card-meta">
                    <HardDrive size={10} strokeWidth={2} />
                    {formatBytes(f.filesize)}
                  </span>
                )}
              </div>
            </button>
          ))}
      </div>

      {/* Actions */}
      <div className="fp-actions">
        <button type="button" className="fp-btn-back" onClick={onBack}>
          <ArrowLeft size={14} strokeWidth={2} />
          Voltar
        </button>
        <button
          type="button"
          className={`fp-btn-download ${!hasSelection || isLoading ? "disabled" : ""}`}
          onClick={handleDownload}
          disabled={!hasSelection || isLoading}
        >
          <Download size={15} strokeWidth={2.5} />
          {isLoading ? "Iniciando..." : "Baixar"}
        </button>
      </div>
    </div>
  );
}
