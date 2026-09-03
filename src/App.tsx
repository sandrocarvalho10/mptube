import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { DownloaderForm } from "./components/DownloaderForm";
import { DownloadList } from "./components/DownloadList";
import { Particles } from "./components/Particles";
import { DownloadItem, DownloadRequest, DownloadStatus } from "./types";
import "./App.css";

function generateId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

// ─── Simulation (dev/no-backend fallback) ────────────────────────────────────

function simulateProgress(
  id: string,
  setItems: React.Dispatch<React.SetStateAction<DownloadItem[]>>
): ReturnType<typeof setInterval> {
  let progress = 0;
  const interval = setInterval(() => {
    progress += Math.random() * 8 + 2;
    if (progress >= 100) {
      progress = 100;
      clearInterval(interval);
      setItems((prev) =>
        prev.map((item) =>
          item.id === id
            ? {
                ...item,
                progress: 100,
                status: "done" as DownloadStatus,
                title: item.title || "Arquivo baixado",
                filePath: `/Users/Downloads/example.${item.format}`,
              }
            : item
        )
      );
      return;
    }
    setItems((prev) =>
      prev.map((item) =>
        item.id === id
          ? {
              ...item,
              progress,
              status: "downloading" as DownloadStatus,
              speed: `${(Math.random() * 4 + 1).toFixed(1)} MiB/s`,
              eta: `00:${String(Math.floor((100 - progress) / 5)).padStart(2, "0")}`,
            }
          : item
      )
    );
  }, 400);
  return interval;
}

// ─── App ─────────────────────────────────────────────────────────────────────

function App() {
  const [items, setItems] = useState<DownloadItem[]>([]);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [simIntervals] = useState<Map<string, ReturnType<typeof setInterval>>>(new Map());

  // Listen to Tauri backend progress events.
  // The Rust backend sends snake_case fields, so we cast to `any` and remap.
  useEffect(() => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const unlisten = listen<any>("download-progress", (event) => {
      const p = event.payload;
      setItems((prev) =>
        prev.map((item) =>
          item.id === p.id
            ? {
                ...item,
                progress: p.progress,
                status: p.status as DownloadStatus,
                speed: p.speed ?? item.speed,
                eta: p.eta ?? item.eta,
                title: p.title ?? item.title,
                filePath: p.file_path ?? p.filePath ?? item.filePath,
                errorMessage: p.error_message ?? p.errorMessage ?? item.errorMessage,
              }
            : item
        )
      );
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const handleDownload = useCallback(
    async (req: DownloadRequest) => {
      setIsSubmitting(true);
      const id = generateId();

      const newItem: DownloadItem = {
        id,
        url: req.url,
        title: "",
        mediaType: req.mediaType,
        format: req.format,
        quality: req.quality,
        status: "fetching_info",
        progress: 0,
        createdAt: Date.now(),
      };

      setItems((prev) => [newItem, ...prev]);
      setIsSubmitting(false);

      try {
        await invoke("start_download", {
          id,
          url: req.url,
          mediaType: req.mediaType,
          format: req.format,
          quality: req.quality,
        });
      } catch (_err) {
        // Backend not yet built — run simulation so UI is fully testable
        console.warn("start_download unavailable, running simulation");
        const handle = simulateProgress(id, setItems);
        simIntervals.set(id, handle);
      }
    },
    [simIntervals]
  );

  const handleCancel = useCallback(
    async (id: string) => {
      // Cancel simulation if running
      const handle = simIntervals.get(id);
      if (handle !== undefined) {
        clearInterval(handle);
        simIntervals.delete(id);
      }

      setItems((prev) =>
        prev.map((item) =>
          item.id === id ? { ...item, status: "cancelled" as DownloadStatus } : item
        )
      );

      try {
        await invoke("cancel_download", { id });
      } catch {
        // Not yet implemented
      }
    },
    [simIntervals]
  );

  const handleRetry = useCallback(
    (id: string) => {
      const item = items.find((i) => i.id === id);
      if (!item) return;
      handleDownload({
        url: item.url,
        mediaType: item.mediaType,
        format: item.format,
        quality: item.quality,
      });
    },
    [items, handleDownload]
  );

  const handleOpen = useCallback(
    async (id: string) => {
      const item = items.find((i) => i.id === id);
      if (!item?.filePath) return;
      try {
        await invoke("open_file", { path: item.filePath });
      } catch {
        console.warn("open_file not available");
      }
    },
    [items]
  );

  const handleClearDone = useCallback(() => {
    setItems((prev) =>
      prev.filter(
        (i) => i.status !== "done" && i.status !== "error" && i.status !== "cancelled"
      )
    );
  }, []);

  return (
    <div className="app-shell">
      <Particles count={45} />
      <div className="app-container">
        <DownloaderForm onDownload={handleDownload} isLoading={isSubmitting} />
        <DownloadList
          items={items}
          onCancel={handleCancel}
          onRetry={handleRetry}
          onOpen={handleOpen}
          onClearDone={handleClearDone}
        />
        <footer className="app-footer">
          Todos direitos reservados | Desenvolvido por <span>SCarvalho</span>
        </footer>
      </div>
    </div>
  );
}

export default App;
