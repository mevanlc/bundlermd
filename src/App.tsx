import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open, save } from "@tauri-apps/plugin-dialog";
import "./App.css";

interface Skipped {
  path: string;
  reason: string;
}

interface AddResult {
  files: string[];
  skipped: Skipped[];
}

interface Problem {
  path: string;
  reason: string;
}

interface ExportResult {
  written: boolean;
  problems: Problem[];
}

interface ContextMenuState {
  x: number;
  y: number;
  path: string;
}

function basename(path: string): string {
  const i = path.lastIndexOf("/");
  return i >= 0 ? path.slice(i + 1) : path;
}

function dirname(path: string): string {
  const i = path.lastIndexOf("/");
  return i > 0 ? path.slice(0, i) : "";
}

export default function App() {
  const [files, setFiles] = useState<string[]>([]);
  const [skipped, setSkipped] = useState<Skipped[]>([]);
  const [problems, setProblems] = useState<Problem[] | null>(null);
  const [pendingExportPath, setPendingExportPath] = useState<string | null>(null);
  const [statusMsg, setStatusMsg] = useState<string>("");
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const dragIndex = useRef<number | null>(null);

  const addPaths = useCallback(async (paths: string[]) => {
    if (paths.length === 0) return;
    const result = await invoke<AddResult>("add_files", { paths });
    setFiles(result.files);
    if (result.skipped.length > 0) setSkipped(result.skipped);
  }, []);

  useEffect(() => {
    invoke<string[]>("get_files").then(setFiles);
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "over") {
        setDragOver(true);
      } else if (event.payload.type === "drop") {
        setDragOver(false);
        void addPaths(event.payload.paths);
      } else {
        setDragOver(false);
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [addPaths]);

  useEffect(() => {
    const close = () => setMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("blur", close);
    };
  }, []);

  async function browse() {
    const picked = await open({ multiple: true, title: "Add Files to Bundle" });
    if (picked) await addPaths(picked);
  }

  async function removeFile(path: string) {
    setFiles(await invoke<string[]>("remove_file", { path }));
  }

  async function moveFile(path: string, op: "up" | "down" | "top" | "bottom") {
    setFiles(await invoke<string[]>("move_file", { path, op }));
  }

  async function runExport(outputPath: string, allowProblems: boolean) {
    try {
      const result = await invoke<ExportResult>("export_bundle", {
        outputPath,
        allowProblems,
      });
      if (result.written) {
        setProblems(null);
        setPendingExportPath(null);
        setStatusMsg(`Exported to ${outputPath}`);
      } else {
        setProblems(result.problems);
        setPendingExportPath(outputPath);
      }
    } catch (e) {
      setStatusMsg(String(e));
    }
  }

  async function exportBundle() {
    const outputPath = await save({
      title: "Export Bundle",
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (!outputPath) return;
    setStatusMsg("");
    await runExport(outputPath, false);
  }

  function onRowDragStart(index: number) {
    dragIndex.current = index;
  }

  async function onRowDrop(index: number) {
    const from = dragIndex.current;
    dragIndex.current = null;
    if (from === null || from === index) return;
    const next = [...files];
    const [moved] = next.splice(from, 1);
    next.splice(index, 0, moved);
    setFiles(next); // optimistic; backend echo below is authoritative
    setFiles(await invoke<string[]>("set_order", { paths: next }));
  }

  return (
    <main className={`app${dragOver ? " drag-over" : ""}`}>
      <header className="toolbar">
        <button onClick={browse}>Add Files…</button>
        <button onClick={exportBundle} disabled={files.length === 0}>
          Export Bundle…
        </button>
      </header>

      {files.length === 0 ? (
        <div className="empty-hint">Drop files here, or use “Add Files…”</div>
      ) : (
        <ul className="file-list">
          {files.map((path, i) => (
            <li
              key={path}
              draggable
              onDragStart={() => onRowDragStart(i)}
              onDragOver={(e) => e.preventDefault()}
              onDrop={() => void onRowDrop(i)}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ x: e.clientX, y: e.clientY, path });
              }}
            >
              <span className="file-name">{basename(path)}</span>
              <span className="file-dir">{dirname(path)}</span>
            </li>
          ))}
        </ul>
      )}

      {statusMsg && <footer className="status">{statusMsg}</footer>}

      {menu && (
        <ul className="context-menu" style={{ left: menu.x, top: menu.y }}>
          <li onClick={() => void moveFile(menu.path, "up")}>Move Up</li>
          <li onClick={() => void moveFile(menu.path, "down")}>Move Down</li>
          <li onClick={() => void moveFile(menu.path, "top")}>Move to Top</li>
          <li onClick={() => void moveFile(menu.path, "bottom")}>Move to Bottom</li>
          <li className="separator" />
          <li onClick={() => void removeFile(menu.path)}>Remove File from Bundle</li>
        </ul>
      )}

      {skipped.length > 0 && (
        <div className="modal-backdrop">
          <div className="modal">
            <h2>Some files were not added</h2>
            <ul className="problem-list">
              {skipped.map((s) => (
                <li key={s.path}>
                  <code>{s.path}</code>
                  <span className="reason">{s.reason}</span>
                </li>
              ))}
            </ul>
            <div className="modal-buttons">
              <button onClick={() => setSkipped([])}>OK</button>
            </div>
          </div>
        </div>
      )}

      {problems && pendingExportPath && (
        <div className="modal-backdrop">
          <div className="modal">
            <h2>Problems during export</h2>
            <p>
              The following files could not be included. Save the export anyway
              (without them), or cancel to fix the issues and retry.
            </p>
            <ul className="problem-list">
              {problems.map((p) => (
                <li key={p.path}>
                  <code>{p.path}</code>
                  <span className="reason">{p.reason}</span>
                </li>
              ))}
            </ul>
            <div className="modal-buttons">
              <button onClick={() => void runExport(pendingExportPath, true)}>
                Save anyway
              </button>
              <button
                onClick={() => {
                  setProblems(null);
                  setPendingExportPath(null);
                }}
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
    </main>
  );
}
