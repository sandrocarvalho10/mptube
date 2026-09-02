import { Trash2 } from "lucide-react";
import { DownloadItem } from "../types";
import { DownloadItemCard } from "./DownloadItemCard";

interface Props {
  items: DownloadItem[];
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onOpen: (id: string) => void;
  onClearDone: () => void;
}

export function DownloadList({ items, onCancel, onRetry, onOpen, onClearDone }: Props) {
  if (items.length === 0) return null;

  const doneCount = items.filter(
    (i) => i.status === "done" || i.status === "error" || i.status === "cancelled"
  ).length;
  const activeCount = items.filter(
    (i) => i.status === "downloading" || i.status === "converting" || i.status === "fetching_info"
  ).length;

  return (
    <section className="download-list">
      <div className="download-list-header">
        <h2 className="list-title">
          Downloads
          {activeCount > 0 && (
            <span className="active-badge">
              {activeCount} ativo{activeCount > 1 ? "s" : ""}
            </span>
          )}
        </h2>
        {doneCount > 0 && (
          <button className="btn-clear" onClick={onClearDone} title="Limpar concluídos">
            <Trash2 size={13} strokeWidth={2} />
            Limpar concluídos
          </button>
        )}
      </div>

      <div className="download-cards">
        {items.map((item) => (
          <DownloadItemCard
            key={item.id}
            item={item}
            onCancel={onCancel}
            onRetry={onRetry}
            onOpen={onOpen}
          />
        ))}
      </div>
    </section>
  );
}
