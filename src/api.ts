// Camada de transporte: fala com o backend via Tauri IPC (app desktop) ou via
// HTTP/WebSocket (versão web), escolhendo em runtime com `isTauri()`.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { DownloadRequest, FetchFormatsResult } from "./types";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type RawProgress = any;

async function readError(res: Response, fallback: string): Promise<string> {
  try {
    const data = await res.json();
    return typeof data?.error === "string" ? data.error : fallback;
  } catch {
    return fallback;
  }
}

export async function fetchFormats(url: string): Promise<FetchFormatsResult> {
  if (isTauri()) {
    return invoke<FetchFormatsResult>("fetch_formats", { url });
  }
  const res = await fetch("/api/formats", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ url }),
  });
  if (!res.ok) throw await readError(res, "Erro ao buscar formatos.");
  return (await res.json()) as FetchFormatsResult;
}

// Web: ids dos downloads desta aba. O servidor só envia progresso dos ids que a
// conexão pediu com `{"watch": [...]}`; reenviamos todos a cada reconexão.
const watchedIds = new Set<string>();
let activeSocket: WebSocket | null = null;

function sendWatch(ids: string[]) {
  if (ids.length === 0) return;
  if (activeSocket?.readyState === WebSocket.OPEN) {
    activeSocket.send(JSON.stringify({ watch: ids }));
  }
}

function watchDownload(id: string) {
  watchedIds.add(id);
  sendWatch([id]);
}

export async function startDownload(id: string, req: DownloadRequest): Promise<void> {
  if (isTauri()) {
    await invoke("start_download", {
      id,
      url: req.url,
      mediaType: req.mediaType,
      format: req.format,
      quality: req.quality,
    });
    return;
  }
  // Assina antes do POST para não perder o primeiro evento ("fetching_info").
  watchDownload(id);
  const res = await fetch("/api/downloads", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      id,
      url: req.url,
      mediaType: req.mediaType,
      format: req.format,
      quality: req.quality,
    }),
  });
  if (!res.ok) throw await readError(res, "Erro ao iniciar download.");
}

export async function cancelDownload(id: string): Promise<void> {
  if (isTauri()) {
    await invoke("cancel_download", { id });
    return;
  }
  await fetch(`/api/downloads/${encodeURIComponent(id)}/cancel`, { method: "POST" });
}

/** Desktop: revela o arquivo no gerenciador de arquivos. Web: inicia o download no navegador. */
export function openOrDownloadFile(item: { id: string; filePath?: string }): void {
  if (isTauri()) {
    if (!item.filePath) return;
    invoke("open_file", { path: item.filePath }).catch(() => {});
    return;
  }
  window.location.href = `/api/downloads/${encodeURIComponent(item.id)}/file`;
}

/** Assina o stream de progresso. Retorna uma função para cancelar a assinatura. */
export function onProgress(callback: (p: RawProgress) => void): () => void {
  if (isTauri()) {
    const unlisten = listen<RawProgress>("download-progress", (event) => callback(event.payload));
    return () => {
      unlisten.then((fn) => fn());
    };
  }

  let socket: WebSocket | null = null;
  let closedByUser = false;
  let retryDelay = 1000;
  let retryTimer: ReturnType<typeof setTimeout> | null = null;

  function connect() {
    const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
    socket = new WebSocket(`${proto}//${window.location.host}/api/ws`);
    activeSocket = socket;
    socket.onmessage = (ev) => {
      try {
        callback(JSON.parse(ev.data));
      } catch {
        // ignora mensagens que não são JSON válido
      }
    };
    socket.onopen = () => {
      retryDelay = 1000;
      sendWatch([...watchedIds]);
    };
    socket.onclose = () => {
      if (closedByUser) return;
      retryTimer = setTimeout(connect, retryDelay);
      retryDelay = Math.min(retryDelay * 2, 15000);
    };
  }

  connect();

  return () => {
    closedByUser = true;
    if (retryTimer) clearTimeout(retryTimer);
    if (activeSocket === socket) activeSocket = null;
    socket?.close();
  };
}
