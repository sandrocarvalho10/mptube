import { useState, useEffect } from "react";
import * as api from "../api";
import {
  ArrowDownToLine,
  Clipboard,
  X,
  Search,
  Loader2,
} from "lucide-react";
import {
  DownloadRequest,
  FetchFormatsResult,
  detectSource,
  SOURCE_LABELS,
} from "../types";
import { FormatPicker } from "./FormatPicker";

interface Props {
  onDownload: (req: DownloadRequest) => void;
  isLoading?: boolean;
}

// Parâmetros de playlist/rádio que devem ser removidos da URL
const PLAYLIST_PARAMS = ["list", "index", "start_radio", "list_type", "ab_channel", "radio"];

function cleanUrl(rawUrl: string): string {
  try {
    const u = new URL(rawUrl);
    PLAYLIST_PARAMS.forEach((p) => u.searchParams.delete(p));
    return u.toString();
  } catch {
    return rawUrl;
  }
}

function isInstagramReel(url: string): boolean {
  return /instagram\.com\/(reels?|reel)\//.test(url);
}

type Step = "input" | "fetching" | "picking";

export function DownloaderForm({ onDownload, isLoading = false }: Props) {
  const [url, setUrl] = useState("");
  const [detectedSource, setDetectedSource] = useState<string | null>(null);
  const [isReel, setIsReel] = useState(false);
  const [urlError, setUrlError] = useState<string | null>(null);

  const [step, setStep] = useState<Step>("input");
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [formatsResult, setFormatsResult] = useState<FetchFormatsResult | null>(null);

  useEffect(() => {
    if (!url.trim()) {
      setDetectedSource(null);
      setIsReel(false);
      setUrlError(null);
      return;
    }
    try {
      new URL(url.trim());
      const src = detectSource(url);
      const reel = isInstagramReel(url);
      setDetectedSource(reel ? "Instagram Reel" : SOURCE_LABELS[src]);
      setIsReel(reel);
      setUrlError(null);
    } catch {
      setDetectedSource(null);
      setIsReel(false);
      setUrlError("URL inválida");
    }
  }, [url]);

  async function handleFetch() {
    if (!url.trim() || urlError) return;
    const cleaned = cleanUrl(url.trim());
    setStep("fetching");
    setFetchError(null);
    try {
      const result = await api.fetchFormats(cleaned);
      setFormatsResult(result);
      setStep("picking");
    } catch (err) {
      setFetchError(typeof err === "string" ? err : "Erro ao buscar formatos.");
      setStep("input");
    }
  }

  function handleBack() {
    setStep("input");
    setFormatsResult(null);
    setFetchError(null);
  }

  function handleSelect(quality: string, mediaType: "video" | "audio", ext: string) {
    if (!formatsResult) return;
    const outputExt = mediaType === "audio" ? "mp3" : ext;
    onDownload({
      url: cleanUrl(url.trim()),
      mediaType,
      format: outputExt as DownloadRequest["format"],
      quality,
      title: formatsResult.title,
      thumbnail: formatsResult.thumbnail,
    });
  }

  function handlePaste() {
    navigator.clipboard.readText().then((text) => {
      if (text) {
        setUrl(text.trim());
        setStep("input");
        setFormatsResult(null);
      }
    });
  }

  const canFetch = url.trim().length > 0 && !urlError && step !== "fetching";

  // ── Etapa 2: seleção de formato ──────────────────────────────────────────
  if (step === "picking" && formatsResult) {
    return (
      <div className="downloader-form">
        <div className="form-header">
          <span className="form-icon">
            <ArrowDownToLine size={26} strokeWidth={2.5} />
          </span>
          <div>
            <h1 className="form-title">mptube</h1>
            <p className="form-subtitle">Escolha o formato para baixar</p>
          </div>
        </div>
        <FormatPicker
          result={formatsResult}
          onSelect={handleSelect}
          onBack={handleBack}
          isLoading={isLoading}
        />
      </div>
    );
  }

  // ── Etapa 1: entrada de URL ───────────────────────────────────────────────
  return (
    <form
      className="downloader-form"
      onSubmit={(e) => { e.preventDefault(); handleFetch(); }}
    >
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
            <span className={`source-badge ${isReel ? "source-badge--reel" : ""}`}>
              {detectedSource}
            </span>
          )}
          <input
            type="text"
            className="url-input"
            placeholder="https://youtube.com/watch?v=... ou instagram.com/reel/..."
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
        {isReel && !urlError && (
          <span className="field-hint">🎬 Reel detectado — escolha Vídeo ou Áudio na próxima etapa</span>
        )}
        {fetchError && <span className="field-error">{fetchError}</span>}
      </div>

      {/* Fetch button */}
      <button
        type="submit"
        className={`btn-download ${step === "fetching" ? "loading" : ""}`}
        disabled={!canFetch}
      >
        {step === "fetching" ? (
          <>
            <Loader2 size={16} strokeWidth={2.5} className="spin-icon" />
            Buscando formatos...
          </>
        ) : (
          <>
            <Search size={16} strokeWidth={2.5} />
            BUSCAR FORMATOS
          </>
        )}
      </button>
    </form>
  );
}
