import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import "./App.css";

interface FileRow {
  path: string;
  name: string;
  folder: string;
  size: number | null;
}

interface Skipped {
  path: string;
  reason: string;
}

interface AddResult {
  files: FileRow[];
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

function formatSize(size: number | null): string {
  if (size === null) return "—";
  if (size < 1024) return `${size} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = size;
  let unit = "B";
  for (const u of units) {
    if (value < 1024) break;
    value /= 1024;
    unit = u;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${unit}`;
}

export default function App() {
  const [files, setFiles] = useState<FileRow[]>([]);
  const [skipped, setSkipped] = useState<Skipped[]>([]);
  const [problems, setProblems] = useState<Problem[] | null>(null);
  const [pendingExportPath, setPendingExportPath] = useState<string | null>(null);
  const [statusMsg, setStatusMsg] = useState<string>("");
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const [dropTarget, setDropTarget] = useState<number | null>(null);
  const dragIndex = useRef<number | null>(null);

  useEffect(() => {
    invoke<FileRow[]>("get_files").then(setFiles);
  }, []);

  useEffect(() => {
    const close = () => setMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("blur", close);
    };
  }, []);

  function applyAddResult(result: AddResult) {
    setFiles(result.files);
    if (result.skipped.length > 0) setSkipped(result.skipped);
  }

  async function browseFiles() {
    const picked = await open({ multiple: true, title: "Add Files to Bundle" });
    if (!picked || picked.length === 0) return;
    applyAddResult(await invoke<AddResult>("add_files", { paths: picked }));
  }

  async function browseFolder() {
    const picked = await open({ directory: true, title: "Add Folder Contents to Bundle" });
    if (!picked) return;
    try {
      applyAddResult(await invoke<AddResult>("add_folder", { path: picked }));
    } catch (e) {
      setStatusMsg(String(e));
    }
  }

  async function removeFile(path: string) {
    setFiles(await invoke<FileRow[]>("remove_file", { path }));
  }

  async function moveFile(path: string, op: "up" | "down" | "top" | "bottom") {
    setFiles(await invoke<FileRow[]>("move_file", { path, op }));
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

  async function onRowDrop(index: number) {
    const from = dragIndex.current;
    dragIndex.current = null;
    setDropTarget(null);
    if (from === null || from === index) return;
    const next = [...files];
    const [moved] = next.splice(from, 1);
    next.splice(index, 0, moved);
    setFiles(next); // optimistic; backend echo below is authoritative
    setFiles(
      await invoke<FileRow[]>("set_order", { paths: next.map((f) => f.path) })
    );
  }

  return (
    <main className="app">
      <header className="toolbar">
        <button onClick={browseFiles}>Add Files…</button>
        <button onClick={browseFolder}>Add Folder…</button>
        <button
          className="export-btn"
          onClick={exportBundle}
          disabled={files.length === 0}
        >
          Export Bundle…
        </button>
      </header>

      {files.length === 0 ? (
        <div className="empty-hint">Use “Add Files…” or “Add Folder…” to get started</div>
      ) : (
        <div className="file-table-wrap">
          <div className="file-table" role="table">
            <div className="ft-row ft-header" role="row">
              <div className="col-name" role="columnheader">Name</div>
              <div className="col-folder" role="columnheader">Folder</div>
              <div className="col-size" role="columnheader">Size</div>
            </div>
            {files.map((file, i) => (
              <div
                key={file.path}
                role="row"
                className={`ft-row${dropTarget === i ? " drop-target" : ""}`}
                draggable
                onDragStart={() => {
                  dragIndex.current = i;
                }}
                onDragOver={(e) => {
                  e.preventDefault();
                  setDropTarget(i);
                }}
                onDragLeave={() => setDropTarget((t) => (t === i ? null : t))}
                onDragEnd={() => setDropTarget(null)}
                onDrop={() => void onRowDrop(i)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenu({ x: e.clientX, y: e.clientY, path: file.path });
                }}
              >
                <div className="col-name" role="cell">
                  <button
                    className="remove-btn"
                    title="Remove from bundle"
                    onClick={() => void removeFile(file.path)}
                  >
                    ✕
                  </button>
                  {file.name}
                </div>
                <div className="col-folder" role="cell" title={file.folder}>
                  {file.folder}
                </div>
                <div className="col-size" role="cell">
                  {formatSize(file.size)}
                </div>
              </div>
            ))}
          </div>
        </div>
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
