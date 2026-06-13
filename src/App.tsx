import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import menuJson from "./menu.json";
import "./App.css";

interface FileRow {
  path: string;
  name: string;
  folder: string;
  size: number | null;
}

type PathPresentation = { mode: "smart" } | { mode: "absolute" };

interface ProjectSettings {
  description: string;
  include_description_in_export: boolean;
  newlines: "unix" | "windows" | "platform";
  path_presentation: PathPresentation;
  toc_links: boolean;
}

interface ProjectView {
  files: FileRow[];
  settings: ProjectSettings;
  project_path: string | null;
  dirty: boolean;
}

interface Skipped {
  path: string;
  reason: string;
}

interface AddResult {
  project: ProjectView;
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

interface FolderPreviewState {
  folder: string;
  recursive: boolean;
  files: string[];
}

interface AppSettings {
  theme: "system" | "light" | "dark";
  max_file_bytes: number;
  max_total_bytes: number;
  menu_rendering: "native" | "both";
  default_project_settings: ProjectSettings;
}

/** Mirrors the node shapes in src/menu.json (shared with the Rust side). */
type MenuItemDef =
  | { separator: true }
  | { predefined: string }
  | { label: string; recents: true }
  | { id: string; label: string; accelerator?: string };

interface MenuDef {
  menus: { label: string; items: MenuItemDef[] }[];
}

const MENU_DEF = menuJson as unknown as MenuDef;

const IS_MAC = navigator.platform.toUpperCase().includes("MAC");

function formatAccel(accelerator?: string): string {
  if (!accelerator) return "";
  const parts = accelerator.split("+");
  if (IS_MAC) {
    const sym: Record<string, string> = {
      CmdOrCtrl: "⌘",
      Cmd: "⌘",
      Ctrl: "⌃",
      Shift: "⇧",
      Alt: "⌥",
      Option: "⌥",
    };
    return parts.map((p) => sym[p] ?? p).join("");
  }
  return parts.map((p) => (p === "CmdOrCtrl" ? "Ctrl" : p)).join("+");
}

/** In-window stand-ins for the native Edit menu's predefined items. */
const PREDEFINED: Record<string, { label: string; accel: string; cmd: string }> = {
  undo: { label: "Undo", accel: "CmdOrCtrl+Z", cmd: "undo" },
  redo: { label: "Redo", accel: "Shift+CmdOrCtrl+Z", cmd: "redo" },
  cut: { label: "Cut", accel: "CmdOrCtrl+X", cmd: "cut" },
  copy: { label: "Copy", accel: "CmdOrCtrl+C", cmd: "copy" },
  paste: { label: "Paste", accel: "CmdOrCtrl+V", cmd: "paste" },
  select_all: { label: "Select All", accel: "CmdOrCtrl+A", cmd: "selectAll" },
};

/** The <li> rows of one menu's dropdown, shared by the in-window menubar and
 *  the macOS toolbar Project button. `fire` runs an action id and closes the
 *  menu; predefined Edit items go through document.execCommand. */
function MenuItems({
  items,
  recents,
  fire,
}: {
  items: MenuItemDef[];
  recents: string[];
  fire: (id: string) => void;
}) {
  return (
    <>
      {items.map((item, j) => {
        if ("separator" in item) return <li key={j} className="separator" />;
        if ("predefined" in item) {
          const p = PREDEFINED[item.predefined];
          return (
            <li key={j} onClick={() => document.execCommand(p.cmd)}>
              <span>{p.label}</span>
              <span className="accel">{formatAccel(p.accel)}</span>
            </li>
          );
        }
        if ("recents" in item) {
          const empty = recents.length === 0;
          return (
            <li key={j} className={`has-sub${empty ? " disabled" : ""}`}>
              <span>{item.label}</span>
              <span className="accel">▸</span>
              {!empty && (
                <ul className="mb-submenu">
                  {recents.map((r) => (
                    <li key={r} onClick={() => fire(`recent:${r}`)}>
                      <span className="recent-path">{r}</span>
                    </li>
                  ))}
                  <li className="separator" />
                  <li onClick={() => fire("clear_recents")}>
                    <span>Clear Menu</span>
                  </li>
                </ul>
              )}
            </li>
          );
        }
        return (
          <li key={j} onClick={() => fire(item.id)}>
            <span>{item.label}</span>
            <span className="accel">{formatAccel(item.accelerator)}</span>
          </li>
        );
      })}
    </>
  );
}

/** Optional in-window menubar (App Settings: "Native + in-window"), rendered
 *  from the same src/menu.json as the native menu and dispatching through
 *  the same action ids. */
function MenuBar({ dispatch }: { dispatch: (id: string) => void }) {
  const [openIdx, setOpenIdx] = useState<number | null>(null);
  const [recents, setRecents] = useState<string[]>([]);

  useEffect(() => {
    if (openIdx !== null) {
      void invoke<string[]>("get_recents").then(setRecents);
    }
  }, [openIdx]);

  useEffect(() => {
    const close = () => setOpenIdx(null);
    window.addEventListener("click", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("blur", close);
    };
  }, []);

  function fire(id: string) {
    setOpenIdx(null);
    dispatch(id);
  }

  return (
    <nav className="menubar">
      {MENU_DEF.menus.map((menu, i) => (
        <div className="mb-menu" key={menu.label}>
          <button
            className={openIdx === i ? "open" : ""}
            onClick={(e) => {
              e.stopPropagation();
              setOpenIdx(openIdx === i ? null : i);
            }}
            onMouseEnter={() => {
              if (openIdx !== null) setOpenIdx(i);
            }}
          >
            {menu.label}
          </button>
          {openIdx === i && (
            <ul className="mb-drop">
              <MenuItems items={menu.items} recents={recents} fire={fire} />
            </ul>
          )}
        </div>
      ))}
    </nav>
  );
}

/** macOS-only toolbar dropdown for the Project menu. On macOS the native menu
 *  bar sits at the top of the screen, away from the window, so this gives a
 *  quick in-window affordance. Renders the "Project" menu from src/menu.json. */
function ProjectMenuButton({ dispatch }: { dispatch: (id: string) => void }) {
  const [open, setOpen] = useState(false);
  const [recents, setRecents] = useState<string[]>([]);
  const projectMenu = MENU_DEF.menus.find((m) => m.label === "Project");

  useEffect(() => {
    if (open) void invoke<string[]>("get_recents").then(setRecents);
  }, [open]);

  useEffect(() => {
    const close = () => setOpen(false);
    window.addEventListener("click", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("blur", close);
    };
  }, []);

  if (!projectMenu) return null;

  function fire(id: string) {
    setOpen(false);
    dispatch(id);
  }

  return (
    <div className="project-menu">
      <button
        className={open ? "open" : ""}
        onClick={(e) => {
          e.stopPropagation();
          setOpen(!open);
        }}
      >
        Project ▾
      </button>
      {open && (
        <ul className="mb-drop">
          <MenuItems items={projectMenu.items} recents={recents} fire={fire} />
        </ul>
      )}
    </div>
  );
}

function InfoTip({ lines }: { lines: string[] }) {
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);

  function show(e: React.SyntheticEvent<HTMLSpanElement>) {
    const r = e.currentTarget.getBoundingClientRect();
    // Anchor above the icon; clamp horizontally to the viewport.
    const width = 304; // 19rem
    const x = Math.max(8, Math.min(r.left + r.width / 2 - width / 2, window.innerWidth - width - 8));
    setPos({ x, y: r.top - 8 });
  }

  return (
    <span
      className="info-tip"
      tabIndex={0}
      aria-label="More info"
      onMouseEnter={show}
      onFocus={show}
      onMouseLeave={() => setPos(null)}
      onBlur={() => setPos(null)}
    >
      ⓘ
      {pos && (
        <span
          className="info-pop"
          role="tooltip"
          style={{ left: pos.x, bottom: window.innerHeight - pos.y }}
        >
          {lines.map((line) => (
            <span key={line}>{line}</span>
          ))}
        </span>
      )}
    </span>
  );
}

function ProjectSettingsEditor({
  draft,
  setDraft,
}: {
  draft: ProjectSettings;
  setDraft: (settings: ProjectSettings) => void;
}) {
  return (
    <>
      <label className="field">
        <span>Description</span>
        <textarea
          rows={5}
          value={draft.description}
          onChange={(e) => setDraft({ ...draft, description: e.target.value })}
        />
      </label>
      <label className="radio">
        <input
          type="checkbox"
          checked={draft.include_description_in_export}
          onChange={(e) =>
            setDraft({
              ...draft,
              include_description_in_export: e.target.checked,
            })
          }
        />
        Include Description in Export
      </label>

      <label className="field">
        <span>
          Path presentation
          <InfoTip
            lines={[
              "How file paths are stored in the project file and shown in the bundle's TOC and headers.",
              "Smart: files in the project file's folder are stored and shown relative to it (so the project travels with its files); everything else stays absolute, shown as the shortest unambiguous name.",
              "Absolute paths: the full path, always.",
            ]}
          />
        </span>
        <select
          value={draft.path_presentation.mode}
          onChange={(e) => {
            const mode = e.target.value as PathPresentation["mode"];
            setDraft({ ...draft, path_presentation: { mode } });
          }}
        >
          <option value="smart">Smart (default)</option>
          <option value="absolute">Absolute paths</option>
        </select>
      </label>

      <label className="field">
        <span>
          Output newlines
          <InfoTip
            lines={[
              "Every newline in the bundle is normalized to this, including inside file content.",
              "Always Unix: LF. Always Windows: CRLF.",
              "Platform Default: whatever the OS running the export uses.",
            ]}
          />
        </span>
        <select
          value={draft.newlines}
          onChange={(e) =>
            setDraft({
              ...draft,
              newlines: e.target.value as ProjectSettings["newlines"],
            })
          }
        >
          <option value="unix">Always Unix</option>
          <option value="windows">Always Windows</option>
          <option value="platform">Platform Default</option>
        </select>
      </label>

      <label className="radio">
        <input
          type="checkbox"
          checked={draft.toc_links}
          onChange={(e) => setDraft({ ...draft, toc_links: e.target.checked })}
        />
        Generate internal links in Table of Contents
      </label>
    </>
  );
}

const EMPTY_PROJECT: ProjectView = {
  files: [],
  settings: {
    description: "",
    include_description_in_export: true,
    newlines: "unix",
    path_presentation: { mode: "smart" },
    toc_links: false,
  },
  project_path: null,
  dirty: false,
};

const BUNDLE_FILTER = [{ name: "BundlerMD Project", extensions: ["bmd"] }];

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

function projectDisplayName(view: ProjectView): string {
  if (!view.project_path) return "Untitled";
  const base = view.project_path.split("/").pop() ?? view.project_path;
  return base.endsWith(".bmd") ? base.slice(0, -4) : base;
}

export default function App() {
  const [project, setProject] = useState<ProjectView>(EMPTY_PROJECT);
  const [skipped, setSkipped] = useState<Skipped[]>([]);
  const [problems, setProblems] = useState<Problem[] | null>(null);
  const [pendingExportPath, setPendingExportPath] = useState<string | null>(null);
  const [statusMsg, setStatusMsg] = useState<string>("");
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const [settingsDraft, setSettingsDraft] = useState<ProjectSettings | null>(null);
  const [folderPreview, setFolderPreview] = useState<FolderPreviewState | null>(null);
  const [appSettings, setAppSettings] = useState<AppSettings | null>(null);
  const [appSettingsDraft, setAppSettingsDraft] = useState<AppSettings | null>(null);
  const [defaultProjectDraft, setDefaultProjectDraft] =
    useState<ProjectSettings | null>(null);
  const [closePromptOpen, setClosePromptOpen] = useState(false);
  const [pendingAction, setPendingAction] = useState<(() => Promise<void>) | null>(null);
  const [dropTarget, setDropTarget] = useState<number | null>(null);
  // macOS convention is a bare document title in the titlebar (the app name
  // already lives in the menu bar); other platforms keep the app-name suffix.
  const [isMacOS, setIsMacOS] = useState(false);
  const dragIndex = useRef<number | null>(null);
  const projectRef = useRef(project);
  projectRef.current = project;

  // Latest handlers for the once-registered menu listener.
  const menuHandlersRef = useRef<Record<string, () => void>>({});
  menuHandlersRef.current = {
    new: newProject,
    new_window: () => void invoke("new_window"),
    open: openProject,
    save: () => void saveProject(false),
    save_as: () => void saveProject(true),
    export: () => void exportBundle(),
    project_settings: () => setSettingsDraft(projectRef.current.settings),
    app_settings: () => {
      if (appSettings) setAppSettingsDraft(appSettings);
    },
    clear_recents: () => void invoke("clear_recents"),
  };

  function dispatchMenu(id: string) {
    if (id.startsWith("recent:")) {
      openRecent(id.slice("recent:".length));
    } else {
      menuHandlersRef.current[id]?.();
    }
  }
  const dispatchMenuRef = useRef(dispatchMenu);
  dispatchMenuRef.current = dispatchMenu;

  useEffect(() => {
    invoke<ProjectView>("get_project").then(setProject);
    invoke<AppSettings>("get_app_settings").then(setAppSettings);
    // Listen on the window (not globally): the backend targets menu/close
    // events at a specific window's label. The settings broadcast reaches
    // every window's listeners regardless of target.
    const appWindow = getCurrentWindow();
    const unlistenClose = appWindow.listen("close-requested", () =>
      setClosePromptOpen(true)
    );
    const unlistenMenu = appWindow.listen<string>("menu", (e) => {
      dispatchMenuRef.current(e.payload);
    });
    const unlistenAppSettings = appWindow.listen<AppSettings>(
      "app-settings-changed",
      (e) => setAppSettings(e.payload)
    );
    return () => {
      void unlistenClose.then((fn) => fn());
      void unlistenMenu.then((fn) => fn());
      void unlistenAppSettings.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    void invoke<string>("host_os").then((os) => setIsMacOS(os === "macos"));
  }, []);

  useEffect(() => {
    const name = projectDisplayName(project);
    const dirtyMark = project.dirty ? " •" : "";
    const suffix = isMacOS ? "" : " — BundlerMD";
    void getCurrentWindow().setTitle(`${name}${dirtyMark}${suffix}`);
  }, [project.project_path, project.dirty, isMacOS]);

  // Missing-file detection: re-poll the backend (which stats every file)
  // about once a second; only re-render when something actually changed.
  // Export-time re-checks remain the functional gate — this is cosmetic.
  useEffect(() => {
    const id = setInterval(() => {
      invoke<ProjectView>("get_project").then((next) => {
        setProject((prev) =>
          JSON.stringify(prev) === JSON.stringify(next) ? prev : next
        );
      });
    }, 1000);
    return () => clearInterval(id);
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

  // Esc dismisses the topmost cancellable overlay.
  const escRef = useRef<() => void>(() => {});
  escRef.current = () => {
    if (defaultProjectDraft) setDefaultProjectDraft(null);
    else if (settingsDraft) setSettingsDraft(null);
    else if (appSettingsDraft) setAppSettingsDraft(null);
    else if (folderPreview) setFolderPreview(null);
    else if (closePromptOpen) setClosePromptOpen(false);
    else if (pendingAction) setPendingAction(null);
    else if (problems) {
      setProblems(null);
      setPendingExportPath(null);
    } else if (skipped.length > 0) setSkipped([]);
    else if (menu) setMenu(null);
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") escRef.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  function applyAddResult(result: AddResult) {
    setProject(result.project);
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
    await loadFolderPreview(picked, false);
  }

  async function loadFolderPreview(folder: string, recursive: boolean) {
    try {
      const files = await invoke<string[]>("preview_folder", { path: folder, recursive });
      setFolderPreview({ folder, recursive, files });
    } catch (e) {
      setFolderPreview(null);
      setStatusMsg(String(e));
    }
  }

  async function confirmFolderAdd() {
    if (!folderPreview) return;
    const paths = folderPreview.files;
    setFolderPreview(null);
    applyAddResult(await invoke<AddResult>("add_files", { paths }));
  }

  async function removeFile(path: string) {
    setProject(await invoke<ProjectView>("remove_file", { path }));
  }

  async function moveFile(path: string, op: "up" | "down" | "top" | "bottom") {
    setProject(await invoke<ProjectView>("move_file", { path, op }));
  }

  /** Save to the current .bmd, or prompt for a location. Returns true if saved. */
  async function saveProject(forceAsk: boolean): Promise<boolean> {
    let path: string | null = null;
    if (forceAsk || !projectRef.current.project_path) {
      path = await save({ title: "Save Project", filters: BUNDLE_FILTER });
      if (!path) return false;
    }
    try {
      setProject(await invoke<ProjectView>("save_project", { path }));
      return true;
    } catch (e) {
      setStatusMsg(String(e));
      return false;
    }
  }

  /** Run `action` now, or stash it behind the save-changes prompt if dirty. */
  function guardDirty(action: () => Promise<void>) {
    if (projectRef.current.dirty) {
      setPendingAction(() => action);
    } else {
      void action();
    }
  }

  async function loadProjectFromPath(path: string) {
    try {
      const view = await invoke<ProjectView | null>("open_project", { path });
      // null: already open in another window, which was focused instead.
      if (view) {
        setProject(view);
        setStatusMsg("");
      }
    } catch (e) {
      setStatusMsg(String(e));
    }
  }

  function openProject() {
    guardDirty(async () => {
      const picked = await open({
        multiple: false,
        title: "Open Project",
        filters: BUNDLE_FILTER,
      });
      if (picked) await loadProjectFromPath(picked);
    });
  }

  function openRecent(path: string) {
    guardDirty(() => loadProjectFromPath(path));
  }

  function newProject() {
    guardDirty(async () => {
      setProject(await invoke<ProjectView>("new_project"));
      setStatusMsg("");
    });
  }

  async function resolvePendingAction(saveFirst: boolean) {
    const action = pendingAction;
    setPendingAction(null);
    if (!action) return;
    if (saveFirst && !(await saveProject(false))) return;
    await action();
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
        setProject(await invoke<ProjectView>("get_project"));
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
    const next = [...project.files];
    const [moved] = next.splice(from, 1);
    next.splice(index, 0, moved);
    setProject({ ...project, files: next }); // optimistic
    setProject(
      await invoke<ProjectView>("set_order", { paths: next.map((f) => f.path) })
    );
  }

  async function commitSettings() {
    if (!settingsDraft) return;
    setProject(await invoke<ProjectView>("update_settings", { settings: settingsDraft }));
    setSettingsDraft(null);
  }

  async function commitAppSettings() {
    if (!appSettingsDraft) return;
    await invoke("set_app_settings", { settings: appSettingsDraft });
    setAppSettings(appSettingsDraft); // broadcast also updates other windows
    setAppSettingsDraft(null);
  }

  function commitDefaultProjectSettings() {
    if (!appSettingsDraft || !defaultProjectDraft) return;
    setAppSettingsDraft({
      ...appSettingsDraft,
      default_project_settings: defaultProjectDraft,
    });
    setDefaultProjectDraft(null);
  }

  async function closeWindowSaving(saveFirst: boolean) {
    if (saveFirst && !(await saveProject(false))) {
      setClosePromptOpen(false);
      return;
    }
    await getCurrentWindow().destroy();
  }

  const draft = settingsDraft;

  return (
    <main className="app" onContextMenu={(e) => e.preventDefault()}>
      {appSettings?.menu_rendering === "both" && <MenuBar dispatch={dispatchMenu} />}
      <header className="toolbar">
        {isMacOS && <ProjectMenuButton dispatch={dispatchMenu} />}
        <button onClick={() => void browseFiles()}>Add Files…</button>
        <button onClick={() => void browseFolder()}>Add Folder…</button>
        <button
          className="gear-btn"
          title="Project Info…"
          aria-label="Project Info"
          onClick={() => setSettingsDraft(project.settings)}
        >
          ⓘ
        </button>
        <button
          className="export-btn"
          onClick={() => void exportBundle()}
          disabled={project.files.length === 0}
        >
          Export Bundle…
        </button>
      </header>

      {project.files.length === 0 ? (
        <div className="empty-hint">Use “Add Files…” or “Add Folder…” to get started</div>
      ) : (
        <div className="file-table-wrap">
          <div className="file-table" role="table">
            <div className="ft-row ft-header" role="row">
              <div className="col-name" role="columnheader">Name</div>
              <div className="col-folder" role="columnheader">Folder</div>
              <div className="col-size" role="columnheader">Size</div>
            </div>
            {project.files.map((file, i) => (
              <div
                key={file.path}
                role="row"
                className={`ft-row${dropTarget === i ? " drop-target" : ""}${
                  file.size === null ? " missing" : ""
                }`}
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
                <div
                  className="col-name"
                  role="cell"
                  title={file.size === null ? "File is missing or unreadable" : undefined}
                >
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

      {draft && (
        <div className="modal-backdrop">
          <div className="modal settings-modal">
            <h2>Project Info</h2>

            <ProjectSettingsEditor
              draft={draft}
              setDraft={(settings) => setSettingsDraft(settings)}
            />

            <div className="modal-buttons">
              <button onClick={() => void commitSettings()}>OK</button>
              <button onClick={() => setSettingsDraft(null)}>Cancel</button>
            </div>
          </div>
        </div>
      )}

      {appSettingsDraft && (
        <div className="modal-backdrop">
          <div className="modal settings-modal">
            <h2>Settings</h2>

            <label className="field">
              <span>
                Theme
                <InfoTip
                  lines={[
                    "Applies to all BundlerMD windows.",
                    "System: follow the OS appearance.",
                  ]}
                />
              </span>
              <select
                value={appSettingsDraft.theme}
                onChange={(e) =>
                  setAppSettingsDraft({
                    ...appSettingsDraft,
                    theme: e.target.value as AppSettings["theme"],
                  })
                }
              >
                <option value="system">System (default)</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </select>
            </label>

            <label className="field">
              <span>
                Menu bar
                <InfoTip
                  lines={[
                    "Native only: the OS menu bar.",
                    "Native + in-window: additionally show a menu bar inside each window. Both render the same menu definition.",
                  ]}
                />
              </span>
              <select
                value={appSettingsDraft.menu_rendering}
                onChange={(e) =>
                  setAppSettingsDraft({
                    ...appSettingsDraft,
                    menu_rendering: e.target.value as AppSettings["menu_rendering"],
                  })
                }
              >
                <option value="native">Native menu only (default)</option>
                <option value="both">Native + in-window menu bar</option>
              </select>
            </label>

            <div className="field">
              <span>Default project</span>
              <button
                type="button"
                onClick={() =>
                  setDefaultProjectDraft(appSettingsDraft.default_project_settings)
                }
              >
                Edit Default Project...
              </button>
            </div>

            <label className="field">
              <span>
                Maximum individual file size (bytes)
                <InfoTip
                  lines={[
                    "Files larger than this are skipped when adding and reported as problems at export.",
                    "Default: 200,000,000 bytes (200 MB).",
                  ]}
                />
              </span>
              <input
                type="number"
                min={1}
                value={appSettingsDraft.max_file_bytes}
                onChange={(e) =>
                  setAppSettingsDraft({
                    ...appSettingsDraft,
                    max_file_bytes: Math.max(1, Number(e.target.value) || 0),
                  })
                }
              />
            </label>

            <label className="field">
              <span>
                Maximum total export size (bytes)
                <InfoTip
                  lines={[
                    "Files that would push the bundle past this are skipped when adding and reported as problems at export.",
                    "Default: 250,000,000 bytes (250 MB).",
                  ]}
                />
              </span>
              <input
                type="number"
                min={1}
                value={appSettingsDraft.max_total_bytes}
                onChange={(e) =>
                  setAppSettingsDraft({
                    ...appSettingsDraft,
                    max_total_bytes: Math.max(1, Number(e.target.value) || 0),
                  })
                }
              />
            </label>

            <div className="modal-buttons">
              <button onClick={() => void commitAppSettings()}>OK</button>
              <button onClick={() => setAppSettingsDraft(null)}>Cancel</button>
            </div>
          </div>
        </div>
      )}

      {defaultProjectDraft && (
        <div className="modal-backdrop">
          <div className="modal settings-modal">
            <h2>Default Project</h2>

            <ProjectSettingsEditor
              draft={defaultProjectDraft}
              setDraft={setDefaultProjectDraft}
            />

            <div className="modal-buttons">
              <button onClick={commitDefaultProjectSettings}>OK</button>
              <button onClick={() => setDefaultProjectDraft(null)}>Cancel</button>
            </div>
          </div>
        </div>
      )}

      {folderPreview && (
        <div className="modal-backdrop">
          <div className="modal folder-preview-modal">
            <h2>Add Folder Contents</h2>
            <p className="folder-preview-path">
              <code>{folderPreview.folder}</code>
            </p>
            <label className="radio">
              <input
                type="checkbox"
                checked={folderPreview.recursive}
                onChange={(e) =>
                  void loadFolderPreview(folderPreview.folder, e.target.checked)
                }
              />
              Include files in subfolders
            </label>
            {folderPreview.files.length === 0 ? (
              <p className="preview-empty">No files found in this folder.</p>
            ) : (
              <>
                <p className="preview-count">
                  {folderPreview.files.length}{" "}
                  {folderPreview.files.length === 1 ? "file" : "files"} will be
                  screened and added:
                </p>
                <ul className="preview-list">
                  {folderPreview.files.map((f) => (
                    <li key={f}>
                      {f.startsWith(folderPreview.folder + "/")
                        ? f.slice(folderPreview.folder.length + 1)
                        : f}
                    </li>
                  ))}
                </ul>
              </>
            )}
            <div className="modal-buttons">
              <button
                onClick={() => void confirmFolderAdd()}
                disabled={folderPreview.files.length === 0}
              >
                Add {folderPreview.files.length > 0 ? folderPreview.files.length : ""}{" "}
                {folderPreview.files.length === 1 ? "File" : "Files"}
              </button>
              <button onClick={() => setFolderPreview(null)}>Cancel</button>
            </div>
          </div>
        </div>
      )}

      {pendingAction && (
        <div className="modal-backdrop">
          <div className="modal">
            <h2>Save changes?</h2>
            <p>
              {projectDisplayName(project)} has unsaved changes. Save them before
              continuing?
            </p>
            <div className="modal-buttons">
              <button onClick={() => void resolvePendingAction(true)}>Save</button>
              <button onClick={() => void resolvePendingAction(false)}>
                Don’t Save
              </button>
              <button onClick={() => setPendingAction(null)}>Cancel</button>
            </div>
          </div>
        </div>
      )}

      {closePromptOpen && (
        <div className="modal-backdrop">
          <div className="modal">
            <h2>Save changes?</h2>
            <p>
              {projectDisplayName(project)} has unsaved changes. Save them before
              closing?
            </p>
            <div className="modal-buttons">
              <button onClick={() => void closeWindowSaving(true)}>Save</button>
              <button onClick={() => void closeWindowSaving(false)}>Don’t Save</button>
              <button onClick={() => setClosePromptOpen(false)}>Cancel</button>
            </div>
          </div>
        </div>
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
