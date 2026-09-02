import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Cookie, FileUp, Trash2, CheckCircle, ExternalLink } from "lucide-react";

export function CookiesConfig() {
  const [cookiePath, setCookiePath] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  // Restaura o path salvo ao montar
  useEffect(() => {
    invoke<string | null>("get_cookies_file")
      .then((p) => setCookiePath(p ?? null))
      .catch(() => {});
  }, []);

  async function handleSelect() {
    setLoading(true);
    try {
      const path = await invoke<string | null>("select_cookies_file");
      if (path) setCookiePath(path);
    } catch {
      // usuário cancelou o picker
    } finally {
      setLoading(false);
    }
  }

  async function handleClear() {
    try {
      await invoke("clear_cookies_file");
      setCookiePath(null);
    } catch {}
  }

  const fileName = cookiePath ? cookiePath.split(/[\\/]/).pop() : null;

  return (
    <div className="cookies-config">
      <div className="cookies-header">
        <span className="cookies-icon">
          <Cookie size={15} strokeWidth={2} />
        </span>
        <div className="cookies-title-group">
          <span className="cookies-title">Cookies</span>
          <span className="cookies-subtitle">
            Necessário para baixar conteúdo com login (YouTube, Instagram…)
          </span>
        </div>

        <a
          className="cookies-help-link"
          href="https://chrome.google.com/webstore/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc"
          target="_blank"
          rel="noopener noreferrer"
          title="Como exportar cookies"
        >
          <ExternalLink size={13} strokeWidth={2} />
          Como exportar
        </a>
      </div>

      {cookiePath ? (
        <div className="cookies-active">
          <CheckCircle size={14} strokeWidth={2.5} className="cookies-check" />
          <span className="cookies-filename" title={cookiePath}>
            {fileName}
          </span>
          <button
            className="cookies-btn cookies-btn-remove"
            onClick={handleClear}
            title="Remover arquivo de cookies"
          >
            <Trash2 size={13} strokeWidth={2} />
            Remover
          </button>
          <button
            className="cookies-btn cookies-btn-change"
            onClick={handleSelect}
            disabled={loading}
            title="Trocar arquivo"
          >
            <FileUp size={13} strokeWidth={2} />
            Trocar
          </button>
        </div>
      ) : (
        <div className="cookies-empty">
          <span className="cookies-hint">
            Sem cookies — downloads públicos funcionam normalmente
          </span>
          <button
            className="cookies-btn cookies-btn-import"
            onClick={handleSelect}
            disabled={loading}
          >
            <FileUp size={13} strokeWidth={2} />
            {loading ? "Abrindo…" : "Importar cookies.txt"}
          </button>
        </div>
      )}
    </div>
  );
}
