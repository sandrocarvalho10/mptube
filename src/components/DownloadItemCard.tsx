import {
  Music,
  Clapperboard,
  Image,
  Smartphone,
  FolderOpen,
  RotateCcw,
  X,
  CheckCircle,
  XCircle,
  Minus,
  Settings,
} from "lucide-react";
import { DownloadItem as DLItem, DownloadStatus } from "../types";

interface Props {
  item: DLItem;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onOpen: (id: string) => void;
}

const STATUS_LABELS: Record<DownloadStatus, string> = {
  queued: "Na fila...",
  fetching_info: "Obtendo informações...",
  downloading: "Baixando",
  converting: "Convertendo...",
  done: "Concluído",
  error: "Erro",
  cancelled: "Cancelado",
};

function MediaIcon({ item }: { item: DLItem }) {
  const props = { size: 20, strokeWidth: 1.8 };
  if (item.mediaType === "audio") return <Music {...props} />;
  if (item.mediaType === "image") return <Image {...props} />;
  if (item.mediaType === "reel")  return <Smartphone {...props} />;
  return <Clapperboard {...props} />;
}

function progressBarColor(status: DownloadStatus): string {
  if (status === "done")      return "var(--color-success)";
  if (status === "error")     return "var(--color-error)";
  if (status === "cancelled") return "var(--color-muted)";
  if (status === "converting") return "var(--color-warning)";
  return "var(--color-accent)";
}

export function DownloadItemCard({ item, onCancel, onRetry, onOpen }: Props) {
  const isActive   = item.status === "downloading" || item.status === "converting" || item.status === "fetching_info";
  const isDone     = item.status === "done";
  const isError    = item.status === "error";
  const isCancelled = item.status === "cancelled";

  const displayTitle = item.title || (() => { try { return new URL(item.url).hostname; } catch { return item.url; } })();
  const formatLabel  = item.format.toUpperCase();

  const progressDisplay =
    item.status === "fetching_info" ? "..." :
    item.status === "converting"    ? <Settings size={13} className="spin-icon" /> :
    `${Math.round(item.progress)}%`;

  return (
    <div className={`download-card ${item.status}`}>
      {/* Card Header */}
      <div className="download-card-header">
        <span className="download-icon">
          <MediaIcon item={item} />
        </span>

        <div className="download-info">
          <span className="download-title" title={displayTitle}>
            {displayTitle}
          </span>
          <span className="download-meta">
            {formatLabel}
            {item.quality && ` · ${item.quality}`}
            {isActive && item.speed && ` · ${item.speed}`}
            {isActive && item.eta && ` · ETA ${item.eta}`}
          </span>
        </div>

        <div className="download-actions">
          {isDone && (
            <button
              className="btn-action btn-open"
              title="Abrir arquivo"
              onClick={() => onOpen(item.id)}
            >
              <FolderOpen size={14} strokeWidth={2} />
            </button>
          )}
          {isError && (
            <button
              className="btn-action btn-retry"
              title="Tentar novamente"
              onClick={() => onRetry(item.id)}
            >
              <RotateCcw size={14} strokeWidth={2} />
            </button>
          )}
          {(isActive || item.status === "queued") && (
            <button
              className="btn-action btn-cancel"
              title="Cancelar"
              onClick={() => onCancel(item.id)}
            >
              <X size={14} strokeWidth={2.5} />
            </button>
          )}
        </div>
      </div>

      {/* Progress Bar */}
      <div className="progress-area">
        <div className="progress-bar-track">
          <div
            className={`progress-bar-fill ${isActive ? "animated" : ""}`}
            style={{
              width: item.status === "fetching_info" ? "100%" : `${item.progress}%`,
              background: progressBarColor(item.status),
              opacity: item.status === "fetching_info" ? 0.4 : 1,
            }}
          />
        </div>
        <span className={`progress-label ${isDone ? "done" : ""} ${isError ? "error" : ""}`}>
          {isDone      ? <CheckCircle size={15} strokeWidth={2.5} /> :
           isError     ? <XCircle     size={15} strokeWidth={2.5} /> :
           isCancelled ? <Minus       size={13} strokeWidth={2.5} /> :
           progressDisplay}
        </span>
      </div>

      {/* Status row */}
      <div className="download-status-row">
        <span className={`status-text status-${item.status}`}>
          {STATUS_LABELS[item.status]}
          {isError && item.errorMessage && `: ${item.errorMessage}`}
        </span>
        {isDone && item.filePath && (
          <span className="file-path" title={item.filePath}>
            {item.filePath.split("/").pop()}
          </span>
        )}
      </div>
    </div>
  );
}
