import { lazy, Suspense, useCallback, useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import * as api from "./lib/api";
import type { AppSnapshot, ComparisonSummary, FindingSummary, ImportDiagnostics, ImportResult, InvestigationBundle, PatchReview, Profile, ProviderDiagnostic, RawInspection, RemediationTask, RepositoryContext, ReportSummary, StorageInfo, TaskState, ValidationCandidate, ValidationRun } from "./lib/types";
import styles from "./styles/App.module.css";

type Message = { tone: "info" | "success" | "error"; text: string } | null;
type Modal = "proposal" | "command" | "provider" | null;
type ResizeSide = "left" | "right";
type WorkflowStage = "triage" | "evidence" | "proposal" | "review" | "validation";
type IconName = "arrowUpRight" | "check" | "close" | "diff" | "external" | "focus" | "package" | "search" | "shieldCheck" | "warning";
type RestoreFocusProp = { restoreFocus?: HTMLElement | null };

const workflowSteps: { id: WorkflowStage; label: string; shortLabel: string }[] = [
  { id: "triage", label: "Triage", shortLabel: "Select" },
  { id: "evidence", label: "Evidence", shortLabel: "Inspect" },
  { id: "proposal", label: "Proposal", shortLabel: "Draft" },
  { id: "review", label: "Review", shortLabel: "Review" },
  { id: "validation", label: "Validation", shortLabel: "Validate" },
];

function Icon({ name, className = "" }: { name: IconName; className?: string }) {
  const props = { className: `${styles.icon} ${className}`.trim(), viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 1.8, strokeLinecap: "round" as const, strokeLinejoin: "round" as const, "aria-hidden": true };
  switch (name) {
    case "arrowUpRight": return <svg {...props}><path d="M7 17 17 7" /><path d="M8 7h9v9" /></svg>;
    case "check": return <svg {...props}><path d="m5 12 4 4L19 6" /></svg>;
    case "close": return <svg {...props}><path d="m6 6 12 12M18 6 6 18" /></svg>;
    case "diff": return <svg {...props}><path d="M5 4h6v16H5zM13 4h6v16h-6z" /><path d="M8 8h1M8 12h1M16 8h1M16 12h1" /></svg>;
    case "external": return <svg {...props}><path d="M14 5h5v5" /><path d="m19 5-8 8" /><path d="M18 13v5a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg>;
    case "focus": return <svg {...props}><circle cx="12" cy="12" r="4" /><path d="M12 3v3M12 18v3M3 12h3M18 12h3" /></svg>;
    case "package": return <svg {...props}><path d="m12 3 8 4.5v9L12 21l-8-4.5v-9L12 3Z" /><path d="m4.5 7.5 7.5 4 7.5-4M12 11.5V21" /></svg>;
    case "search": return <svg {...props}><circle cx="10.8" cy="10.8" r="6.3" /><path d="m16 16 4 4" /></svg>;
    case "shieldCheck": return <svg {...props}><path d="M12 3 19 6v5c0 4.6-3 8-7 10-4-2-7-5.4-7-10V6l7-3Z" /><path d="m8.5 12 2.2 2.2 4.8-5" /></svg>;
    case "warning": return <svg {...props}><path d="m12 4 8 15H4L12 4Z" /><path d="M12 9v4M12 16h.01" /></svg>;
  }
}

function useDialogFocus(onClose: () => void, restoreFocus?: HTMLElement | null) {
  const dialogRef = useRef<HTMLElement>(null);
  const onCloseRef = useRef(onClose);

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    const previous = restoreFocus ?? (document.activeElement instanceof HTMLElement ? document.activeElement : undefined);
    const getFocusable = () => Array.from(dialog.querySelectorAll<HTMLElement>("button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex=\"-1\"])"));

    if (!dialog.contains(document.activeElement)) {
      const initial = dialog.querySelector<HTMLElement>("[autofocus]") ?? getFocusable()[0] ?? dialog;
      initial.focus({ preventScroll: true });
    }

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = getFocusable();
      if (focusable.length === 0) {
        event.preventDefault();
        dialog.focus({ preventScroll: true });
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && (document.activeElement === first || !dialog.contains(document.activeElement))) {
        event.preventDefault();
        last.focus({ preventScroll: true });
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus({ preventScroll: true });
      }
    };

    dialog.addEventListener("keydown", onKeyDown);
    return () => {
      dialog.removeEventListener("keydown", onKeyDown);
      window.requestAnimationFrame(() => {
        if (previous?.isConnected && !previous.closest("[inert]")) previous.focus({ preventScroll: true });
      });
    };
  }, []);

  return dialogRef;
}

function ModalShell({ titleId, onClose, restoreFocus, className = "", children }: { titleId: string; onClose: () => void; restoreFocus?: HTMLElement | null; className?: string; children: ReactNode }) {
  const dialogRef = useDialogFocus(onClose, restoreFocus);
  return <div className={styles.modalBackdrop}><section ref={dialogRef} className={`${styles.modal} ${className}`.trim()} role="dialog" aria-modal="true" aria-labelledby={titleId} tabIndex={-1}>{children}</section></div>;
}

const CodeSurface = lazy(() => import("./components/CodeSurface").then(({ CodeSurface: Component }) => ({ default: Component })));
const DiffSurface = lazy(() => import("./components/CodeSurface").then(({ DiffSurface: Component }) => ({ default: Component })));

// Page size requested for the finding queue. The native query caps a page at 500, and the queue
// labels a full page as "or more" instead of implying the list is complete.
const FINDING_PAGE_SIZE = 200;

const taskLabels: Record<TaskState, string> = {
  investigating: "Investigating",
  proposalready: "Proposal ready",
  applied: "Patched · validation pending",
  locallyvalidated: "Locally validated",
  awaitingrescan: "Awaiting rescan",
  blocked: "Blocked",
  failed: "Failed",
};

const matchLabels: Record<string, string> = {
  matched: "Matched",
  relocatedwithevidence: "Relocated with evidence",
  ambiguous: "Ambiguous",
  unavailable: "Unavailable",
  currentfilediffers: "Current file differs",
  notapplicable: "Not applicable",
};

// Which overlays are mounted, and therefore whether the workbench behind them is inert. Deriving
// both from one place keeps `inert`/`aria-hidden` in step with what is rendered: a stale flag with
// no mounted dialog would leave the window with no interactive content.
export function overlayFlags(input: {
  showBind: boolean;
  report?: ReportSummary;
  review?: PatchReview;
  modal: Modal;
  task?: RemediationTask;
  investigation?: InvestigationBundle;
  commandCandidate?: ValidationCandidate;
  validationOpen: boolean;
  rawInspection?: RawInspection;
  storageOpen: boolean;
  storage?: StorageInfo;
}) {
  const flags = {
    bind: input.showBind && Boolean(input.report),
    reviewDialog: Boolean(input.review),
    proposal: input.modal === "proposal" && Boolean(input.task && input.investigation),
    command: input.modal === "command" && Boolean(input.commandCandidate && input.task),
    provider: input.modal === "provider",
    validation: input.validationOpen,
    raw: Boolean(input.rawInspection),
    storageDialog: input.storageOpen && Boolean(input.storage),
  };
  return {
    ...flags,
    blocking: flags.bind || flags.reviewDialog || flags.proposal || flags.command || flags.provider || flags.validation || flags.raw || flags.storageDialog,
  };
}

function App() {
  const [snapshot, setSnapshot] = useState<AppSnapshot>({ findings: [], tasks: [], provider: emptyProvider() });
  const [report, setReport] = useState<ReportSummary>();
  const [diagnostics, setDiagnostics] = useState<ImportDiagnostics>();
  const [profile, setProfile] = useState<Profile>();
  const [repository, setRepository] = useState<RepositoryContext>();
  const [findings, setFindings] = useState<FindingSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string>();
  const [investigation, setInvestigation] = useState<InvestigationBundle>();
  const [task, setTask] = useState<RemediationTask>();
  const [review, setReview] = useState<PatchReview>();
  const [reviewed, setReviewed] = useState(false);
  const [validationCandidates, setValidationCandidates] = useState<ValidationCandidate[]>([]);
  const [validationRuns, setValidationRuns] = useState<ValidationRun[]>([]);
  const [validationOpen, setValidationOpen] = useState(false);
  const [validationLoading, setValidationLoading] = useState(false);
  const [validationError, setValidationError] = useState<string>();
  const [provider, setProvider] = useState<ProviderDiagnostic>(emptyProvider());
  const [comparison, setComparison] = useState<ComparisonSummary>();
  const [rawInspection, setRawInspection] = useState<RawInspection>();
  const [storageOpen, setStorageOpen] = useState(false);
  const [storage, setStorage] = useState<StorageInfo>();
  const [message, setMessage] = useState<Message>(null);
  const [modal, setModal] = useState<Modal>(null);
  const [commandCandidate, setCommandCandidate] = useState<ValidationCandidate>();
  const [repoPath, setRepoPath] = useState("");
  const [scanPrefix, setScanPrefix] = useState("");
  const [repositoryPrefix, setRepositoryPrefix] = useState("");
  const [showBind, setShowBind] = useState(false);
  const [search, setSearch] = useState("");
  const [severityFilter, setSeverityFilter] = useState("");
  const [engineFilter, setEngineFilter] = useState("");
  const [statusFilter, setStatusFilter] = useState("");
  const [savedView, setSavedView] = useState("");
  const [fontSize, setFontSize] = useState(13);
  const [theme, setTheme] = useState<"light" | "dark">("dark");
  const [leftWidth, setLeftWidth] = useState(300);
  const [rightWidth, setRightWidth] = useState(340);
  const searchRef = useRef<HTMLInputElement>(null);
  const workspaceRef = useRef<HTMLDivElement>(null);
  const resizing = useRef<ResizeSide | undefined>(undefined);
  const refreshRequestId = useRef(0);
  const findingsRequestId = useRef(0);
  const investigationRequestId = useRef(0);
  const validationRequestId = useRef(0);
  const overlayReturnFocus = useRef<HTMLElement | null>(null);
  const selectedFindingRef = useRef(selectedId);
  const searchValueRef = useRef(search);
  const [loading, setLoading] = useState(false);
  selectedFindingRef.current = selectedId;
  searchValueRef.current = search;

  const rememberOverlayTrigger = () => {
    const active = document.activeElement;
    if (active instanceof HTMLElement && active !== document.body && active !== document.documentElement) overlayReturnFocus.current = active;
  };

  const rememberPointerTrigger = (event: ReactPointerEvent<HTMLElement>) => {
    if (!(event.target instanceof HTMLElement)) return;
    const trigger = event.target.closest<HTMLElement>('button, a[href], input, select, textarea, [tabindex]:not([tabindex="-1"])');
    if (trigger) overlayReturnFocus.current = trigger;
  };

  const notify = useCallback((tone: "info" | "success" | "error", text: string) => {
    setMessage({ tone, text });
  }, []);

  const refresh = useCallback(async () => {
    const requestId = refreshRequestId.current + 1;
    refreshRequestId.current = requestId;
    const selectionAtStart = selectedFindingRef.current;
    const searchAtStart = searchValueRef.current;
    try {
      const next = await api.loadSnapshot();
      if (requestId !== refreshRequestId.current) return;
      setSnapshot(next);
      setReport(next.report);
      setDiagnostics(next.diagnostics);
      setProfile(next.profile);
      setRepository(next.repository);
      setFindings(next.findings);
      setProvider(next.provider);
      if (selectedFindingRef.current === selectionAtStart && next.profile?.uiState.selectedFindingId) setSelectedId(next.profile.uiState.selectedFindingId);
      if (searchValueRef.current === searchAtStart && next.profile?.uiState.search) setSearch(next.profile.uiState.search);
      if (next.profile?.uiState.theme === "light" || next.profile?.uiState.theme === "dark") setTheme(next.profile.uiState.theme);
      if (next.profile?.uiState.codeFontSize) setFontSize(next.profile.uiState.codeFontSize);
      if (typeof next.profile?.uiState.leftWidth === "number") setLeftWidth(clamp(next.profile.uiState.leftWidth, 270, 520));
      if (typeof next.profile?.uiState.rightWidth === "number") setRightWidth(clamp(next.profile.uiState.rightWidth, 330, 520));
      const selectedTask = next.tasks.find((item) => item.findingIds.includes(next.profile?.uiState.selectedFindingId ?? ""));
      if (selectedTask) setTask(selectedTask);
    } catch (error) {
      if (requestId === refreshRequestId.current) notify("error", errorMessage(error));
    }
  }, [notify]);

  useEffect(() => {
    // The browser preview has no persisted native workspace to hydrate. Keeping it
    // intentionally quiet makes the empty state useful instead of looking broken.
    if (!api.isNative) return;
    void refresh();
  }, [refresh]);

  const applyImport = useCallback((result: ImportResult) => {
    setReport(result.report);
    setDiagnostics(result.diagnostics);
    setFindings(result.findings);
    setComparison(result.comparison);
    setSelectedId(result.findings[0]?.id);
    setInvestigation(undefined);
    setTask(undefined);
    setReview(undefined);
    // A new report invalidates every dialog that was bound to the previous task, including the
    // command approval that is suspended behind the validation drawer.
    setModal(null);
    setCommandCandidate(undefined);
    setShowBind(!repository);
  }, [repository]);

  const acceptDroppedPaths = useCallback(async (paths: string[]) => {
    const candidate = paths[0];
    if (!candidate) return;
    if (candidate.toLowerCase().endsWith(".json")) {
      await importPath(candidate);
    } else {
      setRepoPath(candidate);
      setShowBind(true);
      notify("info", "Repository folder selected. Confirm the mapping before source access is enabled.");
    }
    async function importPath(path: string) {
      setLoading(true);
      try {
        const result = await api.importReport(path);
        applyImport(result);
        notify("success", `Imported ${result.report.sourceName}: ${result.diagnostics.parsedInstances} individual finding instances retained.`);
      } catch (error) { notify("error", errorMessage(error)); } finally { setLoading(false); }
    }
  }, [applyImport, notify]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    if (api.isNative) {
      void getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === "drop") void acceptDroppedPaths(event.payload.paths);
      }).then((dispose) => { if (disposed) dispose(); else unlisten = dispose; }).catch((error) => notify("error", errorMessage(error)));
    } else {
      const onDrop = (event: DragEvent) => {
        event.preventDefault();
        const path = event.dataTransfer?.files[0]?.name;
        if (path) notify("info", "Browser preview received a drop, but native file paths are only available in the Tauri app.");
      };
      window.addEventListener("dragover", preventDefault);
      window.addEventListener("drop", onDrop);
      return () => { window.removeEventListener("dragover", preventDefault); window.removeEventListener("drop", onDrop); };
    }
    return () => { disposed = true; unlisten?.(); };
  }, [acceptDroppedPaths, notify]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") { event.preventDefault(); searchRef.current?.focus(); return; }
      const target = event.target;
      // The list shortcuts stay out of text entry, the read-only code surface, and any dialog.
      if (event.defaultPrevented || target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement || (target instanceof HTMLElement && target.closest('[role="dialog"], [contenteditable="true"], [role="textbox"], .cm-editor')) || !findings.length) return;
      if (event.key === "ArrowDown" || event.key.toLowerCase() === "j") { event.preventDefault(); moveFinding(1); }
      if (event.key === "ArrowUp" || event.key.toLowerCase() === "k") { event.preventDefault(); moveFinding(-1); }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
    function moveFinding(direction: number) {
      const index = findings.findIndex((finding) => finding.id === selectedId);
      const next = findings[Math.max(0, Math.min(findings.length - 1, (index < 0 ? 0 : index) + direction))];
      if (next) setSelectedId(next.id);
    }
  }, [findings, selectedId]);

  useEffect(() => {
    const requestId = findingsRequestId.current + 1;
    findingsRequestId.current = requestId;
    let cancelled = false;
    if (!report) {
      setFindings([]);
      return () => { cancelled = true; };
    }
    const timer = window.setTimeout(() => {
      void api.listFindings({ reportId: report.id, search, severity: severityFilter || undefined, engine: engineFilter || undefined, status: statusFilter || undefined, savedView: savedView || undefined, page: 0, pageSize: FINDING_PAGE_SIZE }).then((next) => {
        if (!cancelled && requestId === findingsRequestId.current) setFindings(next);
      }).catch((error) => {
        if (!cancelled && requestId === findingsRequestId.current) notify("error", errorMessage(error));
      });
    }, 180);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [report, search, severityFilter, engineFilter, statusFilter, savedView, notify]);

  useEffect(() => {
    const matchingTask = selectedId ? snapshot.tasks.find((item) => item.findingIds.includes(selectedId)) : undefined;
    setTask(matchingTask);
    setReview(undefined);
    setReviewed(false);
  }, [selectedId, snapshot.tasks]);

  useEffect(() => {
    const requestId = investigationRequestId.current + 1;
    investigationRequestId.current = requestId;
    let cancelled = false;
    if (!selectedId) {
      setInvestigation(undefined);
      return () => { cancelled = true; };
    }
    setInvestigation(undefined);
    void api.investigation(selectedId).then((bundle) => {
      if (cancelled || requestId !== investigationRequestId.current) return;
      setInvestigation(bundle);
      setRepoPath(repository?.path ?? profile?.repositoryPath ?? "");
    }).catch((error) => {
      if (!cancelled && requestId === investigationRequestId.current) notify("error", errorMessage(error));
    });
    return () => { cancelled = true; };
  }, [selectedId, notify, profile?.repositoryPath, repository?.path]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    const themeColor = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]');
    const previousThemeColor = themeColor?.content;
    if (themeColor) themeColor.content = theme === "light" ? "#efede3" : "#302f2c";
    return () => {
      delete document.documentElement.dataset.theme;
      if (themeColor && previousThemeColor) themeColor.content = previousThemeColor;
    };
  }, [theme]);

  useEffect(() => {
    if (!profile || !api.isNative) return;
    const state = { ...profile.uiState, selectedFindingId: selectedId, search, severityFilter: severityFilter || undefined, engineFilter: engineFilter || undefined, statusFilter: statusFilter || undefined, savedView: savedView || undefined, theme, codeFontSize: fontSize, leftWidth, rightWidth };
    const timer = window.setTimeout(() => { void api.saveUiState(profile.id, state).catch(() => undefined); }, 350);
    return () => window.clearTimeout(timer);
  }, [profile, selectedId, search, severityFilter, engineFilter, statusFilter, savedView, theme, fontSize, leftWidth, rightWidth]);

  useEffect(() => {
    const onMove = (event: PointerEvent) => {
      const side = resizing.current;
      const bounds = workspaceRef.current?.getBoundingClientRect();
      if (!side || !bounds) return;
      if (side === "left") {
        const maximum = Math.max(270, bounds.width - rightWidth - 16 - 380);
        setLeftWidth(clamp(Math.round(event.clientX - bounds.left), 270, maximum));
      } else {
        const maximum = Math.max(330, bounds.width - leftWidth - 16 - 380);
        setRightWidth(clamp(Math.round(bounds.right - event.clientX), 330, maximum));
      }
    };
    const onUp = () => { resizing.current = undefined; document.body.style.cursor = ""; };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    return () => { window.removeEventListener("pointermove", onMove); window.removeEventListener("pointerup", onUp); document.body.style.cursor = ""; };
  }, [leftWidth, rightWidth]);

  const beginResize = (side: ResizeSide, event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    resizing.current = side;
    event.currentTarget.setPointerCapture(event.pointerId);
    document.body.style.cursor = "col-resize";
  };

  const keyboardResize = (side: ResizeSide, event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const delta = event.key === "ArrowLeft" ? -16 : 16;
    const bounds = workspaceRef.current?.getBoundingClientRect();
    if (!bounds) return;
    if (side === "left") {
      const maximum = Math.max(270, bounds.width - rightWidth - 16 - 380);
      setLeftWidth((value) => clamp(value + delta, 270, maximum));
    } else {
      const maximum = Math.max(330, bounds.width - leftWidth - 16 - 380);
      setRightWidth((value) => clamp(value - delta, 330, maximum));
    }
  };

  const openReport = async () => {
    rememberOverlayTrigger();
    setLoading(true);
    try {
      const path = await api.chooseReport();
      if (!path) { if (!api.isNative) notify("info", "Run `pnpm tauri dev` to use native file selection."); return; }
      applyImport(await api.importReport(path));
      notify("success", "Report imported as an immutable snapshot; inspect diagnostics before trusting the parsed count.");
    } catch (error) { notify("error", errorMessage(error)); } finally { setLoading(false); }
  };

  const chooseRepo = async () => {
    rememberOverlayTrigger();
    try {
      const path = await api.chooseRepository();
      if (!path) { if (!api.isNative) notify("info", "Run `pnpm tauri dev` to use native folder selection."); return; }
      setRepoPath(path); setShowBind(true);
    } catch (error) { notify("error", errorMessage(error)); }
  };

  const bindRepo = async () => {
    if (!report || !repoPath) return;
    const findingId = selectedId;
    setLoading(true);
    try {
      const context = await api.bindRepository({ reportId: report.id, repositoryPath: repoPath, scanPrefix: scanPrefix || undefined, repositoryPrefix: repositoryPrefix || undefined });
      setRepository(context); setShowBind(false); await refresh(); notify("success", `Bound ${context.canonicalPath}. ${context.isGitRepository ? "Git-aware review and apply are available." : "Git-backed apply is unavailable; export remains available."}`);
      if (findingId) {
        const bundle = await api.investigation(findingId);
        if (selectedFindingRef.current === findingId) setInvestigation(bundle);
      }
    } catch (error) { notify("error", errorMessage(error)); } finally { setLoading(false); }
  };

  const ensureTask = async (): Promise<RemediationTask | undefined> => {
    if (!selectedId) return undefined;
    const findingId = selectedId;
    const existing = snapshot.tasks.find((item) => item.findingIds.includes(findingId)) ?? (task?.findingIds.includes(findingId) ? task : undefined);
    if (existing) { setTask(existing); return existing; }
    try {
      const created = await api.createTask(findingId);
      setSnapshot((previous) => ({ ...previous, tasks: previous.tasks.some((item) => item.id === created.id) ? previous.tasks : [created, ...previous.tasks] }));
      if (selectedFindingRef.current !== findingId) return undefined;
      setTask(created);
      return created;
    } catch (error) { notify("error", errorMessage(error)); return undefined; }
  };

  const investigate = async () => {
    const created = await ensureTask();
    if (created) notify("success", "Task snapshot captured. Current source is still read-only until a reviewed proposal is applied.");
  };

  const suggestFix = async () => {
    const created = await ensureTask();
    if (created) notify("info", `Offline playbook ready: ${investigation?.playbook.title ?? "contextual investigation"}. It is guidance, not an executable repair.`);
  };

  const reviewChanges = async () => {
    rememberOverlayTrigger();
    const findingId = selectedId;
    const current = task ?? await ensureTask();
    if (!current || !findingId) return;
    try {
      const next = await api.reviewPatch(current.id);
      if (selectedFindingRef.current !== findingId) return;
      setReview(next);
      setReviewed(false);
      notify("info", "Review the complete patch. Applying remains a separate approval and will recheck every base hash.");
    } catch (error) { if (selectedFindingRef.current === findingId) notify("error", errorMessage(error)); }
  };

  const startProposal = async () => {
    rememberOverlayTrigger();
    const current = task ?? await ensureTask();
    if (current) setModal("proposal");
  };

  const runProviderProposal = async () => {
    rememberOverlayTrigger();
    const current = task ?? await ensureTask();
    if (!current || !provider.integrationEnabled) return;
    setLoading(true);
    try {
      const next = await api.runCodexProposal(current.id);
      setReview(next);
      setReviewed(false);
      setTask(await api.getTask(current.id));
      setModal(null);
      notify("success", "Codex returned a schema-validated proposal bound to this task snapshot. Review it before applying.");
    } catch (error) { notify("error", errorMessage(error)); } finally { setLoading(false); }
  };

  const importProposal = async () => {
    rememberOverlayTrigger();
    const current = task ?? await ensureTask();
    if (!current) return;
    try {
      const path = await api.chooseProposal();
      if (!path) return;
      const next = await api.importProposal(current.id, path);
      setTask(await api.getTask(current.id));
      setReview(next);
      setReviewed(false);
      notify("success", "External proposal imported as a bound, un-applied proposal. Review it before any write.");
    } catch (error) { notify("error", errorMessage(error)); }
  };

  const exportTask = async () => {
    const current = task ?? await ensureTask();
    if (!current) return;
    try {
      const path = await api.chooseExportPath(`cxview-${current.id}.json`);
      if (!path) return;
      await api.exportTask(current.id, path);
      notify("success", `Task bundle exported to ${path}. Review proprietary source and scanner evidence before sharing.`);
    } catch (error) { notify("error", errorMessage(error)); }
  };

  const copyPrompt = async () => {
    if (!selectedId) return;
    try { const prompt = await api.investigationPrompt(selectedId); await navigator.clipboard.writeText(prompt); notify("success", "Investigation prompt copied. It contains bounded evidence and a proposal contract, not authorization to write."); } catch (error) { notify("error", errorMessage(error)); }
  };

  const inspectRaw = async (locator: string) => {
    rememberOverlayTrigger();
    if (!report) return;
    const findingId = selectedId;
    try {
      const inspection = await api.readRawLocator(report.id, locator);
      if (selectedFindingRef.current === findingId) setRawInspection(inspection);
    } catch (error) { if (selectedFindingRef.current === findingId) notify("error", errorMessage(error)); }
  };

  const openStorage = async () => {
    rememberOverlayTrigger();
    try { setStorage(await api.storageInfo()); setStorageOpen(true); } catch (error) { notify("error", errorMessage(error)); }
  };

  const deleteWorkspace = async (confirmation: string) => {
    if (!profile) return;
    try { setStorage(await api.deleteWorkspaceData(profile.id, confirmation)); setStorageOpen(false); setReport(undefined); setDiagnostics(undefined); setProfile(undefined); setRepository(undefined); setFindings([]); setSelectedId(undefined); setInvestigation(undefined); setTask(undefined); setComparison(undefined); notify("success", "Workspace-local report, task, patch, note, and validation data were deleted. This was not forensic secure deletion."); } catch (error) { notify("error", errorMessage(error)); }
  };

  const applyPatch = async () => {
    if (!task || !review || !reviewed) return;
    try { const result = await api.applyReviewedPatch({ taskId: task.id, patchId: review.patchId, reviewed: true }); const next = await api.getTask(task.id); setTask(next); setReview(undefined); setSnapshot((previous) => ({ ...previous, tasks: previous.tasks.map((item) => item.id === next.id ? next : item) })); notify("success", `${result.message} The scanner axis remains awaiting rescan.`); } catch (error) { notify("error", errorMessage(error)); }
  };

  const undoPatch = async () => {
    if (!task) return;
    try { const files = await api.undoPatch(task.id); setTask(await api.getTask(task.id)); notify("success", `Undid the CXView patch for ${files.join(", ")}. Subsequent edits were protected by post-image checks.`); } catch (error) { notify("error", errorMessage(error)); }
  };

  const openValidation = async () => {
    rememberOverlayTrigger();
    const current = task ?? await ensureTask();
    if (!current) return;
    const requestId = validationRequestId.current + 1;
    validationRequestId.current = requestId;
    setValidationCandidates([]);
    setValidationRuns([]);
    setValidationError(undefined);
    setValidationLoading(true);
    setValidationOpen(true);
    try {
      const [candidates, runs] = await Promise.all([api.discoverValidation(current.id), api.validationHistory(current.id)]);
      if (requestId !== validationRequestId.current) return;
      setValidationCandidates(candidates);
      setValidationRuns(runs);
    } catch (error) {
      if (requestId !== validationRequestId.current) return;
      const detail = errorMessage(error);
      setValidationError(detail);
      notify("error", detail);
    } finally {
      if (requestId === validationRequestId.current) setValidationLoading(false);
    }
  };

  const confirmCommand = async () => {
    if (!task || !commandCandidate) return;
    setModal(null);
    try { const run = await api.runValidation({ taskId: task.id, candidateId: commandCandidate.id, approved: true }); setValidationRuns((previous) => [run, ...previous]); setTask(await api.getTask(task.id)); notify(run.status === "passed" ? "success" : "error", run.note); } catch (error) { notify("error", errorMessage(error)); }
  };

  const selected = findings.find((finding) => finding.id === selectedId);
  const activeFilters = [severityFilter && `severity: ${severityFilter}`, engineFilter && `engine: ${engineFilter}`, statusFilter && `status: ${statusFilter}`, savedView && `view: ${viewLabel(savedView)}`].filter(Boolean) as string[];
  const overlays = overlayFlags({ showBind, report, review, modal, task, investigation, commandCandidate, validationOpen, rawInspection, storageOpen, storage });
  const bindOpen = overlays.bind;
  const proposalOpen = overlays.proposal;
  const commandOpen = overlays.command;
  const providerOpen = overlays.provider;
  const storageDialogOpen = overlays.storageDialog;
  const hasBlockingOverlay = overlays.blocking;
  const workflowStage: WorkflowStage = validationOpen || task?.patchId ? "validation" : review || task?.proposal ? "review" : task ? "proposal" : selected ? "evidence" : "triage";
  const workflowIndex = workflowSteps.findIndex((step) => step.id === workflowStage);

  return (
    <main className={styles.app} data-theme={theme} aria-labelledby={report ? "workbench-title" : "welcome-title"} onPointerDownCapture={rememberPointerTrigger}>
      <div className={styles.appContent} aria-hidden={hasBlockingOverlay ? "true" : undefined} inert={hasBlockingOverlay ? true : undefined}>
        <header className={styles.topbar}>
        <div className={styles.brandBlock}><div className={styles.logoMark}>CX</div><div><div className={styles.brand}>CXView</div><div className={styles.eyebrow}>LOCAL REMEDIATION WORKBENCH</div></div></div>
        <div className={styles.workspaceCrumb}>{report ? report.sourceName : "Open report + select repository"}<span className={styles.dot}>·</span>{repository ? shortPath(repository.canonicalPath) : "No repository bound"}</div>
        <div className={styles.topActions}><button className={styles.ghostButton} onClick={() => setTheme(theme === "dark" ? "light" : "dark")} aria-label="Toggle light and dark theme">{theme === "dark" ? "Light" : "Dark"}</button><label className={styles.fontControl}>Code <input type="range" min="11" max="18" value={fontSize} onChange={(event) => setFontSize(Number(event.target.value))} aria-label="Code text size" /></label><button className={styles.ghostButton} onClick={() => void openStorage()}>Storage</button><button className={styles.ghostButton} onClick={() => void refresh()} disabled={!api.isNative} title={api.isNative ? "Reload the native workspace" : "Workspace refresh is available in the desktop app"}>Refresh</button></div>
        </header>

      {message && <div className={`${styles.notice} ${styles[`notice${capitalize(message.tone)}`]}`} role={message.tone === "error" ? "alert" : "status"}><span>{message.text}</span><button className={styles.noticeClose} onClick={() => setMessage(null)} aria-label="Dismiss message"><Icon name="close" /></button></div>}
      {report && <h1 id="workbench-title" className={styles.srOnly}>CXView remediation workbench</h1>}

      {!report ? <EmptyState onOpen={openReport} loading={loading} isNative={api.isNative} /> : <>
        <div className={styles.subbar}><div className={styles.reportMeta}><span className={styles.pill}>Snapshot {report.sha256.slice(0, 12)}</span><span>{report.findingCount} parsed instances</span><span>{report.adapterId} v{report.adapterVersion}</span>{report.metadata.branch && <span>branch {report.metadata.branch}</span>}</div><div className={styles.subActions}><button className={styles.secondaryButton} onClick={openReport}>Import later report</button><button className={styles.secondaryButton} onClick={chooseRepo}>Bind repository</button></div></div>
        <nav className={styles.workflowBar} aria-label="Remediation workflow">
          <div className={styles.workflowLead}><strong>Traceable remediation</strong></div>
          <ol className={styles.workflowSteps}>{workflowSteps.map((step, index) => {
            const complete = index < workflowIndex;
            const current = index === workflowIndex;
            return <li key={step.id} className={`${styles.workflowStep} ${complete ? styles.workflowComplete : ""} ${current ? styles.workflowCurrent : ""}`} aria-current={current ? "step" : undefined}><span className={styles.workflowMarker}>{complete ? <Icon name="check" /> : index + 1}</span><span><b>{step.label}</b><small>{step.shortLabel}</small></span></li>;
          })}</ol>
          <span className={styles.workflowState}>{workflowStage === "triage" ? "Select a finding to begin" : workflowSteps[workflowIndex]?.label}</span>
        </nav>
        <div ref={workspaceRef} className={styles.workspace} style={{ gridTemplateColumns: `${leftWidth}px 8px minmax(380px, 1fr) 8px ${rightWidth}px` }}>
          <aside className={`${styles.pane} ${styles.queuePane}`} aria-label="Finding work queue">
            <div className={styles.paneHeader}><div><h2>Findings</h2><span className={styles.paneSubtitle}>{findings.length === 0 ? "No findings in this view" : `${findings.length}${findings.length === FINDING_PAGE_SIZE ? " or more" : ""} in current view`}</span></div><span className={styles.countBadge}>{findings.length}{findings.length === FINDING_PAGE_SIZE ? "+" : ""}</span></div>
            <div className={styles.searchWrap}><span className={styles.searchIcon}><Icon name="search" /></span><input ref={searchRef} value={search} onChange={(event) => setSearch(event.target.value)} placeholder="Search rule, file, package…" aria-label="Search findings" /><kbd>Ctrl/⌘ K</kbd></div>
            <div className={styles.filterGrid}><select value={severityFilter} onChange={(event) => setSeverityFilter(event.target.value)} aria-label="Filter severity"><option value="">All severity</option><option>Critical</option><option>High</option><option>Medium</option><option>Low</option><option>Informational</option><option>Unknown</option></select><select value={engineFilter} onChange={(event) => setEngineFilter(event.target.value)} aria-label="Filter scanner engine"><option value="">All engines</option><option>SAST</option><option>SCA</option><option>IaC</option><option>Unknown</option></select></div>
            <select className={styles.savedView} value={savedView} onChange={(event) => setSavedView(event.target.value)} aria-label="Saved finding view"><option value="">All findings</option><option value="needs-investigation">Needs investigation</option><option value="ready-to-review">Ready to review</option><option value="patched-awaiting-validation">Patched awaiting validation</option><option value="awaiting-rescan">Awaiting rescan</option></select>
            {activeFilters.length > 0 && <div className={styles.activeFilters}><span>Active</span>{activeFilters.map((filter) => <button key={filter} onClick={() => clearFilter(filter, setSeverityFilter, setEngineFilter, setStatusFilter, setSavedView)}>{filter}<Icon name="close" className={styles.filterClearIcon} /></button>)}</div>}
            <div className={styles.findingList}>{findings.map((finding) => <FindingRow key={finding.id} finding={finding} selected={finding.id === selectedId} onSelect={() => setSelectedId(finding.id)} />)}{findings.length === 0 && <div className={styles.emptyList}><strong>No matching findings</strong><span>Clear a filter or inspect import diagnostics. A zero result is not treated as a failed import.</span></div>}</div>
            <div className={styles.queueFooter}><span>Keyboard: ↑↓ / J K</span><span>{diagnostics?.unsupportedRecords.length ?? 0} unsupported retained</span></div>
          </aside>

          <div className={styles.paneDivider} role="separator" aria-orientation="vertical" aria-label="Resize findings pane" aria-valuemin={270} aria-valuemax={520} aria-valuenow={leftWidth} tabIndex={0} onPointerDown={(event) => beginResize("left", event)} onKeyDown={(event) => keyboardResize("left", event)} />

          <section className={`${styles.pane} ${styles.evidencePane}`} aria-label="Evidence and code">
            {selected && !investigation ? <LoadingState label="Loading evidence" detail="Rechecking the selected finding against the current workspace." /> : !investigation || !selected ? <UnselectedState hasRepository={Boolean(repository)} onBind={chooseRepo} /> : <EvidencePane investigation={investigation} fontSize={fontSize} />}
          </section>

          <div className={styles.paneDivider} role="separator" aria-orientation="vertical" aria-label="Resize remediation pane" aria-valuemin={330} aria-valuemax={520} aria-valuenow={rightWidth} tabIndex={0} onPointerDown={(event) => beginResize("right", event)} onKeyDown={(event) => keyboardResize("right", event)} />

          <aside className={`${styles.pane} ${styles.remediationPane}`} aria-label="Remediation workspace">
            {selected && !investigation ? <LoadingState label="Preparing actions" detail="Waiting for the same evidence bundle before enabling remediation context." /> : !investigation || !selected ? <div className={styles.remediationEmpty}><div className={styles.bigIcon}><Icon name="arrowUpRight" /></div><h2>Investigation starts here</h2><p>Select a finding to see the exact scanner claim, current-code observations, unknowns, and an actionable playbook.</p></div> : <RemediationPane investigation={investigation} task={task} provider={provider} onInvestigate={() => void investigate()} onSuggest={() => void suggestFix()} onProposal={() => void startProposal()} onImport={importProposal} onReview={() => void reviewChanges()} onExport={() => void exportTask()} onCopy={() => void copyPrompt()} onRunChecks={() => void openValidation()} onUndo={() => void undoPatch()} onProvider={() => { rememberOverlayTrigger(); setModal("provider"); }} onInspectRaw={(locator) => void inspectRaw(locator)} onNote={(note) => { void api.saveNote(investigation.finding.id, note).catch((error) => notify("error", errorMessage(error))); }} />}
          </aside>
        </div>
        {diagnostics && <DiagnosticsStrip diagnostics={diagnostics} report={report} comparison={comparison} />}
      </>}
      </div>
      {bindOpen && report && <BindRepositoryDialog report={report} path={repoPath} scanPrefix={scanPrefix} repositoryPrefix={repositoryPrefix} restoreFocus={overlayReturnFocus.current} onPathChange={setRepoPath} onScanPrefixChange={setScanPrefix} onRepositoryPrefixChange={setRepositoryPrefix} onBind={() => void bindRepo()} onCancel={() => setShowBind(false)} />}
      {overlays.reviewDialog && review && <ReviewDialog review={review} reviewed={reviewed} onReviewed={setReviewed} onApply={() => void applyPatch()} onClose={() => setReview(undefined)} repository={repository} restoreFocus={overlayReturnFocus.current} />}
      {proposalOpen && task && investigation && <ProposalDialog task={task} investigation={investigation} restoreFocus={overlayReturnFocus.current} onClose={() => setModal(null)} onError={(text) => notify("error", text)} onCreated={(next) => { rememberOverlayTrigger(); setReview(next); setReviewed(false); void api.getTask(task.id).then(setTask).catch(() => undefined); setModal(null); notify("success", "Proposal buffer converted into a real, precondition-checked diff. Review it before applying."); }} />}
      {commandOpen && commandCandidate && task && <CommandDialog candidate={commandCandidate} restoreFocus={overlayReturnFocus.current} onCancel={() => setModal(null)} onApprove={() => void confirmCommand()} />}
      {providerOpen && <ProviderDialog provider={provider} hasTask={Boolean(task)} restoreFocus={overlayReturnFocus.current} onClose={() => setModal(null)} onGenerate={() => void runProviderProposal()} />}
      {overlays.validation && <ValidationDrawer candidates={validationCandidates} runs={validationRuns} loading={validationLoading} error={validationError} suspended={modal === "command"} restoreFocus={overlayReturnFocus.current} onClose={() => { validationRequestId.current += 1; setValidationOpen(false); }} onRetry={() => void openValidation()} onRun={(candidate, trigger) => { overlayReturnFocus.current = trigger; setCommandCandidate(candidate); setModal("command"); }} />}
      {overlays.raw && rawInspection && <RawInspectionDialog inspection={rawInspection} restoreFocus={overlayReturnFocus.current} onClose={() => setRawInspection(undefined)} />}
      {storageDialogOpen && storage && <StorageDialog profile={profile} info={storage} restoreFocus={overlayReturnFocus.current} onClose={() => setStorageOpen(false)} onDelete={(confirmation) => void deleteWorkspace(confirmation)} />}
    </main>
  );
}

function EmptyState({ onOpen, loading, isNative }: { onOpen: () => void; loading: boolean; isNative: boolean }) {
  return <section className={styles.emptyState} aria-labelledby="welcome-title"><div className={styles.emptyOrb}><Icon name="shieldCheck" /></div><h1 id="welcome-title">Evidence before edits.</h1><p>Open a supported Checkmarx One JSON report, bind the repository it came from, and carry one finding through investigation, review, application, and local validation.</p><div className={styles.emptyActions}><button className={styles.primaryButton} onClick={onOpen} disabled={loading}>{loading ? "Importing…" : "Open report"}</button><span>{isNative ? "or drag a JSON report onto this window" : "native file selection runs in the desktop app"}</span></div><div className={styles.previewNote}><span className={styles.previewDot} aria-hidden="true" />{isNative ? "Desktop mode · native file and repository checks available" : "Browser preview · open the desktop app for native file and repository access"}</div><div className={styles.capabilityRow}><span>Offline import + guidance</span><span>Native path checks</span><span>Reviewed patch journal</span><span>Local validation evidence</span></div></section>;
}

function LoadingState({ label, detail }: { label: string; detail: string }) {
  return <div className={styles.loadingState} role="status" aria-live="polite"><div className={styles.loadingMark} aria-hidden="true"><span /><span /><span /></div><strong>{label}</strong><p>{detail}</p></div>;
}

function FindingRow({ finding, selected, onSelect }: { finding: FindingSummary; selected: boolean; onSelect: () => void }) {
  return <button className={`${styles.findingRow} ${selected ? styles.findingSelected : ""}`} onClick={onSelect} aria-current={selected ? "true" : undefined}><div className={styles.findingTop}><span className={`${styles.severity} ${severityClass(finding.severity)}`}>{finding.severity}</span><span className={styles.engineLabel}>{finding.category.toUpperCase()}</span><span className={styles.readiness}>{readinessLabel(finding.evidenceReadiness)}</span></div><strong>{finding.title}</strong><span className={styles.findingLocation}>{finding.filePath ?? finding.packageName ?? "No location supplied"}{finding.lineStart ? `:${finding.lineStart}` : ""}</span><div className={styles.findingBottom}><span>{finding.rule ?? "Rule unknown"}</span><span>{taskLabels[finding.localTaskState]}</span></div></button>;
}

function EvidencePane({ investigation, fontSize }: { investigation: InvestigationBundle; fontSize: number }) {
  const { finding, context } = investigation;
  const [jumpToken, setJumpToken] = useState(0);
  const focusLine = context.currentRangeStart && finding.lineStart ? Math.max(1, finding.lineStart - context.currentRangeStart + 1) : undefined;
  return <div className={styles.paneContent}><div className={styles.evidenceTop}><div className={styles.evidenceHeader}><div><h2>{finding.filePath ?? finding.packageName ?? finding.resource ?? "No location supplied"}</h2><div className={styles.locationLine}>{finding.lineStart ? `reported line ${finding.lineStart}${finding.lineEnd && finding.lineEnd !== finding.lineStart ? `–${finding.lineEnd}` : ""}` : "location not supplied"}<span className={`${styles.stateBadge} ${stateClass(context.matchState)}`}>{matchLabels[context.matchState] ?? context.matchState}</span></div></div><button className={styles.ghostButton} onClick={() => setJumpToken((value) => value + 1)} disabled={!context.currentSource || !focusLine} title="Source navigation is native and read-only">Jump to reported location</button></div><div className={styles.provenanceBar} aria-label="Evidence provenance"><span className={styles.provenanceScanner}><i />Scanner claim</span><span className={styles.provenanceObserved}><i />Observed locally</span><span className={styles.provenanceUnknown}><i />Not established</span><span className={styles.readOnlyMark}>Read-only source</span></div><div className={styles.evidenceTabs}><span className={styles.activeTab}>Current source</span><span>Reported flow · {context.reportedNodes.length} step{context.reportedNodes.length === 1 ? "" : "s"}</span><span>Related files · {context.relatedFiles.length}</span></div></div>{context.currentSource ? <div className={styles.codeFrame}><div className={styles.codeFrameBar}><span>{context.relativePath ?? finding.filePath}</span><span>SHA {context.sourceHash?.slice(0, 12) ?? "unknown"}</span></div><Suspense fallback={<div className={styles.codeLoading}>Loading source…</div>}><CodeSurface content={context.currentSource} filePath={context.relativePath} fontSize={fontSize} focusLine={focusLine} focusToken={jumpToken} /></Suspense></div> : <div className={styles.noSource}><span className={styles.warningGlyph}><Icon name="warning" /></span><div><strong>Current source is not available for editing</strong><p>{context.observedLocal.map((statement) => statement.text).join(" ")}</p>{context.reportedSnippet && <pre>{context.reportedSnippet}</pre>}</div></div>}{context.reportedNodes.length > 0 && <div className={styles.flowPanel}><div className={styles.sectionTitle}>Reported source / sink / flow steps <span>scanner evidence</span></div><div className={styles.flowList}>{context.reportedNodes.map((node) => <div className={styles.flowRow} key={`${node.locator}-${node.order}`}><span className={styles.flowNumber}>{node.order + 1}</span><div><strong>{node.role ?? node.sourceSink ?? "reported node"}</strong><span>{node.path ?? "path not supplied"}{node.lineStart ? `:${node.lineStart}` : ""}</span>{node.snippet && <code>{node.snippet}</code>}</div></div>)}</div></div>}{finding.category === "sca" && context.sca && <DependencyCard dependency={context.sca} />}</div>;
}

function DependencyCard({ dependency }: { dependency: NonNullable<InvestigationBundle["context"]["sca"]> }) {
  return <div className={styles.dependencyCard}><div className={styles.sectionTitle}>Dependency context <span>observed locally</span></div><div className={styles.dependencySummary}><span className={styles.packageIcon}><Icon name="package" /></span><div><strong>{dependency.packageName ?? "package identity unknown"}</strong><span>scan version {dependency.scanVersion ?? "unknown"} · {dependency.directDependency === true ? "direct" : dependency.directDependency === false ? "transitive" : "ownership unknown"}</span></div></div>{dependency.resolvedInstances.length > 0 && <div className={styles.instanceList}>{dependency.resolvedInstances.map((instance) => <div key={`${instance.installPath}-${instance.version}`}><code>{instance.installPath}</code><span>{instance.version ?? "version unknown"}</span></div>)}</div>}{dependency.ownershipPaths.length > 0 && <div className={styles.mutedList}>{dependency.ownershipPaths.slice(0, 8).map((path) => <span key={path}>{path}</span>)}</div>}{dependency.notes.map((note) => <div className={styles.inlineWarning} key={note}><span className={styles.inlineWarningIcon}><Icon name="warning" /></span>{note}</div>)}{dependency.inspectionCommands.length > 0 && <details className={styles.commandDetails}><summary>Reviewed inspection commands</summary>{dependency.inspectionCommands.map((command) => <code key={command}>{command}</code>)}</details>}</div>;
}

function RemediationPane({ investigation, task, provider, onInvestigate, onSuggest, onProposal, onImport, onReview, onExport, onCopy, onRunChecks, onUndo, onProvider, onInspectRaw, onNote }: { investigation: InvestigationBundle; task?: RemediationTask; provider: ProviderDiagnostic; onInvestigate: () => void; onSuggest: () => void; onProposal: () => void; onImport: () => void; onReview: () => void; onExport: () => void; onCopy: () => void; onRunChecks: () => void; onUndo: () => void; onProvider: () => void; onInspectRaw: (locator: string) => void; onNote: (note: string) => void }) {
  const { finding, context, playbook } = investigation;
  const [note, setNote] = useState(investigation.localNote ?? "");
  useEffect(() => setNote(investigation.localNote ?? ""), [investigation.localNote, finding.id]);
  const readiness = task?.patchId ? "Run an approved check, then compare a later scan" : task?.proposal ? "Review the complete diff before any write" : task ? "Draft a proposal from this captured snapshot" : "Capture this finding before drafting a change";
  return <div className={styles.paneContent}><div className={styles.remediationTop}><div className={styles.remediationHeader}><div><h2>Make this finding explainable</h2><span className={styles.remediationSubtitle}>Actions stay bounded to this finding and its evidence snapshot.</span></div>{task && <span className={`${styles.stateBadge} ${taskStateClass(task.state)}`}>{taskLabels[task.state]}</span>}</div><div className={styles.actionReadiness}><span className={styles.labelMuted}>NEXT SAFE ACTION</span><strong>{readiness}</strong><span>Source remains read-only until a reviewed patch is explicitly approved.</span></div><div className={styles.actionGrid}><button className={styles.primaryButton} onClick={onInvestigate}>Investigate</button><button className={styles.secondaryButton} onClick={onSuggest}>Suggest fix</button><button className={styles.secondaryButton} onClick={onReview} disabled={!task?.proposal}>Review changes</button><button className={styles.secondaryButton} onClick={onRunChecks} disabled={!task}>Run checks</button></div></div><div className={styles.disclosure}><span className={styles.labelScanner}>SCANNER EVIDENCE</span><strong>What was reported</strong><p>{finding.description ?? "No description was supplied by the report."}</p><div className={styles.factGrid}><span>Engine <b>{finding.engine}</b></span><span>Original severity <b>{finding.originalSeverity ?? "Unknown"}</b></span><span>Result status <b>{finding.resultStatus ?? "Unknown"}</b></span><span>Triage state <b>{finding.triageState ?? "Unknown"}</b></span><span>Rule/query <b>{finding.rule ?? "Unknown"}</b></span><span>Raw locator <b>{finding.rawLocator} <button className={styles.rawInspectButton} onClick={() => onInspectRaw(finding.rawLocator)}>Inspect JSON</button></b></span></div></div><div className={styles.disclosure}><span className={styles.labelObserved}>OBSERVED LOCALLY</span><strong>What current code shows</strong>{context.observedLocal.map((statement) => <p key={`${statement.text}-${statement.locator}`}>{statement.text}</p>)}{context.syntax && <div className={styles.syntaxMeta}><span>{context.syntax.parser} {context.syntax.parserVersion}</span><span>{context.syntax.syntaxErrorCount} syntax error{context.syntax.syntaxErrorCount === 1 ? "" : "s"}</span></div>}</div><div className={styles.disclosure}><span className={styles.labelUnknown}>NOT ESTABLISHED</span><strong>Unknowns and drift</strong><ul>{context.unknowns.length > 0 ? context.unknowns.map((unknown) => <li key={unknown}>{unknown}</li>) : <li>No additional unknowns recorded; absence of unknowns is not proof of safety.</li>}</ul></div><div className={styles.playbook}><div className={styles.playbookHeading}><div><h3>{playbook.title}</h3><span className={styles.playbookMeta}>Offline guidance · v1</span></div><span>reviewed {playbook.lastReviewed}</span></div><p className={styles.playbookNote}>{playbook.note}</p><PlaybookSection title="Ask first" values={playbook.investigationQuestions} /><PlaybookSection title="Preferred repair" values={playbook.preferredRepairs} /><PlaybookSection title="Preserve + test" values={[...playbook.behaviorPreservation, ...playbook.regressionTests]} /><PlaybookSection title="Reject cosmetic fixes" values={playbook.contraindications} warning />{playbook.references.map((reference) => <a className={styles.reference} key={reference.url} href={reference.url} target="_blank" rel="noreferrer"><Icon name="external" /> {reference.label}</a>)}</div><div className={styles.noteBlock}><div className={styles.sectionTitle}>Local assessment <span>user supplied</span></div><textarea value={note} onChange={(event) => setNote(event.target.value)} onBlur={() => onNote(note)} placeholder="Record a false-positive rationale or investigation note…" maxLength={32768} /></div><div className={styles.proposalTools}><div className={styles.sectionTitle}>Proposal exchange <span>never auto-applies</span></div><div className={styles.toolButtons}><button className={styles.primaryButton} onClick={onProposal}>Edit proposal buffer</button><button className={styles.secondaryButton} onClick={onImport}>Import proposal</button><button className={styles.secondaryButton} onClick={onExport}>Export task</button><button className={styles.secondaryButton} onClick={onCopy}>Copy investigation prompt</button></div>{task?.proposal && <div className={styles.proposalState}><span className={styles.successDot} /> {task.proposal.source} proposal bound to snapshot · {task.proposal.edits.length} edit{task.proposal.edits.length === 1 ? "" : "s"}<button className={styles.linkButton} onClick={onReview}>Open diff</button></div>}</div>{task?.patchId && <div className={styles.appliedCard}><div><span className={styles.labelObserved}>PATCH JOURNAL</span><strong>{task.state === "awaitingrescan" ? "Applied and locally validated" : "Applied"}</strong><p>Scanner status is separate. CXView will not mark this finding fixed until a comparable later export says so.</p></div><button className={styles.secondaryButton} onClick={onUndo}>Undo this CXView patch</button></div>}<button className={styles.providerCard} onClick={onProvider}><span className={`${provider.integrationEnabled ? styles.providerLive : styles.providerOff} ${styles.providerStatus}`} aria-hidden="true" /><div><strong>Optional Codex proposal adapter</strong><span>{provider.diagnostic}</span></div><em>{provider.integrationEnabled ? "available" : "disabled"}</em></button></div>;
}

function PlaybookSection({ title, values, warning = false }: { title: string; values: string[]; warning?: boolean }) { return <div className={styles.playbookSection}><span className={warning ? styles.labelWarning : styles.labelMuted}>{title}</span>{values.map((value) => <p className={warning ? styles.warningText : ""} key={value}>{value}</p>)}</div>; }

function DiagnosticsStrip({ diagnostics, report, comparison }: { diagnostics: ImportDiagnostics; report: ReportSummary; comparison?: ComparisonSummary }) {
  return <section className={styles.diagnostics}><div className={styles.diagnosticsTop}><div><strong>{diagnostics.format}</strong><span>adapter {diagnostics.adapterId} v{diagnostics.adapterVersion} · {report.sha256.slice(0, 16)}</span></div><div className={styles.diagnosticCounts}><span><b>{diagnostics.parsedByEngine.sast}</b> SAST</span><span><b>{diagnostics.parsedByEngine.sca}</b> SCA</span><span><b>{diagnostics.parsedByEngine.iac}</b> IaC</span><span><b>{diagnostics.unsupportedRecords.length}</b> unsupported</span><span><b>{diagnostics.malformedRecords.length}</b> malformed</span></div></div>{(diagnostics.countMismatches.length > 0 || diagnostics.warnings.length > 0 || diagnostics.evidenceNotes.length > 0) && <details><summary>Evidence and compatibility notes</summary><div className={styles.diagnosticGrid}>{diagnostics.evidenceNotes.map((note) => <p key={note}>{note}</p>)}{diagnostics.warnings.map((warning) => <p key={warning} className={styles.warningText}>{warning}</p>)}{diagnostics.countMismatches.map((mismatch) => <p key={mismatch.locator} className={styles.warningText}>Count mismatch at {mismatch.locator}: declared {mismatch.declared}, parsed {mismatch.parsed}.</p>)}</div></details>}{diagnostics.unsupportedRecords.length > 0 && <details><summary>Unsupported records retained for raw inspection ({diagnostics.unsupportedRecords.length})</summary><div className={styles.rawList}>{diagnostics.unsupportedRecords.slice(0, 20).map((record) => <div key={record.locator}><code>{record.locator}</code><span>{record.reason}</span><pre>{record.preview}</pre></div>)}</div></details>}{comparison && <div className={styles.comparisonBar}><span className={comparison.provenanceComparable ? styles.successDot : styles.warningGlyph}>{comparison.provenanceComparable ? <Icon name="check" /> : <Icon name="warning" />}</span><strong>Later report comparison</strong><span>{comparison.stillObserved} still observed · {comparison.absentUnderComparableScope} absent under comparable scope · {comparison.incomparable} incomparable</span><em>{comparison.provenanceReason}</em></div>}</section>;
}

function RawInspectionDialog({ inspection, onClose, restoreFocus }: { inspection: RawInspection; onClose: () => void } & RestoreFocusProp) {
  return <ModalShell titleId="raw-title" onClose={onClose} restoreFocus={restoreFocus}><div className={styles.modalHeader}><div><h2 id="raw-title">Raw JSON inspection</h2></div><button className={styles.iconButton} onClick={onClose} aria-label="Close"><Icon name="close" /></button></div><p><code>{inspection.locator}</code> from the retained imported snapshot. This is report evidence, not a current-code observation.</p><pre className={styles.rawInspection}>{JSON.stringify(inspection.value, null, 2)}</pre><div className={styles.modalActions}><button className={styles.secondaryButton} onClick={onClose}>Close</button></div></ModalShell>;
}

function StorageDialog({ profile, info, onClose, onDelete, restoreFocus }: { profile?: Profile; info: StorageInfo; onClose: () => void; onDelete: (confirmation: string) => void } & RestoreFocusProp) {
  const [confirmation, setConfirmation] = useState("");
  const expected = profile ? `DELETE ${profile.id}` : "";
  return <ModalShell titleId="storage-title" onClose={onClose} restoreFocus={restoreFocus}><div className={styles.modalHeader}><div><h2 id="storage-title">Storage and retention</h2></div><button className={styles.iconButton} onClick={onClose} aria-label="Close"><Icon name="close" /></button></div><div className={styles.storageSummary}><strong>{formatBytes(info.bytes)}</strong><span>{info.retainedObjects.length} retained app-data objects</span><code>{info.root}</code></div><p>{info.note} Reports, snapshots, diffs, and validation output can contain proprietary code or secrets; review exports before sharing.</p>{profile ? <><p>This deletes the active workspace profile <code>{profile.id}</code>, its linked report snapshots, tasks, notes, patch journals, and validation history. It does not delete or reset repository files.</p><label>Type <code>{expected}</code> to confirm<input autoFocus value={confirmation} onChange={(event) => setConfirmation(event.target.value)} /></label><div className={styles.modalActions}><button className={styles.secondaryButton} onClick={onClose}>Cancel</button><button className={styles.dangerButton} disabled={confirmation !== expected} onClick={() => onDelete(confirmation)}>Delete workspace data</button></div></> : <div className={styles.emptyList}><strong>No active workspace profile</strong><span>Import and bind a report/repository before workspace deletion is available.</span></div>}</ModalShell>;
}

function BindRepositoryDialog({ report, path, scanPrefix, repositoryPrefix, onPathChange, onScanPrefixChange, onRepositoryPrefixChange, onBind, onCancel, restoreFocus }: { report: ReportSummary; path: string; scanPrefix: string; repositoryPrefix: string; onPathChange: (value: string) => void; onScanPrefixChange: (value: string) => void; onRepositoryPrefixChange: (value: string) => void; onBind: () => void; onCancel: () => void } & RestoreFocusProp) {
  return <ModalShell titleId="bind-title" onClose={onCancel} restoreFocus={restoreFocus}><div className={styles.modalHeader}><div><h2 id="bind-title">Bind the scanned repository</h2></div><button className={styles.iconButton} onClick={onCancel} aria-label="Close"><Icon name="close" /></button></div><p>Choose the local folder that should be inspected for <strong>{report.sourceName}</strong>. This grants read access for the active workspace; it does not authorize patches.</p><label>Repository folder<input autoFocus value={path} onChange={(event) => onPathChange(event.target.value)} placeholder="Select a folder…" /></label><div className={styles.mappingGrid}><label>Scan prefix <input value={scanPrefix} onChange={(event) => onScanPrefixChange(event.target.value)} placeholder="e.g. C:\\agent\\repo" /></label><label>Repository prefix <input value={repositoryPrefix} onChange={(event) => onRepositoryPrefixChange(event.target.value)} placeholder="e.g. packages/web" /></label></div><p className={styles.modalHint}>A prefix mapping is saved as evidence and never selects a file by basename. Ambiguous or unavailable paths remain non-editable.</p><div className={styles.modalActions}><button className={styles.secondaryButton} onClick={onCancel}>Cancel</button><button className={styles.primaryButton} onClick={onBind} disabled={!path}>Confirm repository</button></div></ModalShell>;
}

function ProposalDialog({ task, investigation, onClose, onCreated, onError, restoreFocus }: { task: RemediationTask; investigation: InvestigationBundle; onClose: () => void; onCreated: (review: PatchReview) => void; onError: (message: string) => void } & RestoreFocusProp) {
  const defaultPath = investigation.context.relativePath ?? investigation.finding.filePath ?? task.snapshot.files[0]?.path ?? "";
  const [diagnosis, setDiagnosis] = useState(`Manual proposal for ${investigation.finding.title}`);
  const [path, setPath] = useState(defaultPath);
  // The anchor is intentionally empty: the snapshot pre-fill was a no-op, and an empty anchor
  // keeps the reviewer pasting the exact span instead of accepting a generated one.
  const [oldText, setOldText] = useState("");
  const [newText, setNewText] = useState("");
  const [behavior, setBehavior] = useState(investigation.playbook.behaviorPreservation[0] ?? "");
  const [tests, setTests] = useState(investigation.playbook.regressionTests[0] ?? "");
  const [expectedAbsent, setExpectedAbsent] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const submit = async () => {
    setSubmitting(true);
    try { const review = await api.createManualProposal(task.id, { diagnosis, assumptions: [], edits: [{ path, oldText, newText, expectedAbsent }], behaviorPreservation: behavior ? [behavior] : [], suggestedTests: tests ? [tests] : [], unresolvedQuestions: [], evidenceRefs: [investigation.finding.rawLocator] }); onCreated(review); } catch (error) { onError(errorMessage(error)); } finally { setSubmitting(false); }
  };
  return <ModalShell titleId="proposal-title" className={styles.proposalModal} onClose={onClose} restoreFocus={restoreFocus}><div className={styles.modalHeader}><div><h2 id="proposal-title">Prepare a candidate repair</h2></div><button className={styles.iconButton} onClick={onClose} aria-label="Close"><Icon name="close" /></button></div><p>These exact text anchors are validated against the captured snapshot. The live file remains read-only until the resulting diff is reviewed and approved.</p><label>Diagnosis<textarea value={diagnosis} onChange={(event) => setDiagnosis(event.target.value)} /></label><div className={styles.mappingGrid}><label>Relative target path<input value={path} onChange={(event) => setPath(event.target.value)} placeholder="src/file.tsx" /></label><label className={styles.checkboxLabel}><input type="checkbox" checked={expectedAbsent} onChange={(event) => setExpectedAbsent(event.target.checked)} /> Create a new file (explicit absence required)</label></div><label>Exact old text anchor {expectedAbsent && <span className={styles.labelMuted}>must be empty</span>}<textarea className={styles.codeInput} value={oldText} onChange={(event) => setOldText(event.target.value)} placeholder={expectedAbsent ? "Leave empty for a new file" : "Paste one unique exact span from the current snapshot"} spellCheck={false} /></label><label>Proposed new text<textarea className={styles.codeInput} value={newText} onChange={(event) => setNewText(event.target.value)} placeholder="The reviewed replacement text" spellCheck={false} /></label><div className={styles.mappingGrid}><label>Behavior rationale<textarea value={behavior} onChange={(event) => setBehavior(event.target.value)} /></label><label>Suggested regression test<textarea value={tests} onChange={(event) => setTests(event.target.value)} /></label></div><div className={styles.modalActions}><button className={styles.secondaryButton} onClick={onClose}>Cancel</button><button className={styles.primaryButton} onClick={() => void submit()} disabled={submitting || !path || (!expectedAbsent && !oldText) || newText === ""}>{submitting ? "Checking anchors…" : "Create reviewed diff"}</button></div></ModalShell>;
}

function ReviewDialog({ review, reviewed, onReviewed, onApply, onClose, repository, restoreFocus }: { review: PatchReview; reviewed: boolean; onReviewed: (value: boolean) => void; onApply: () => void; onClose: () => void; repository?: RepositoryContext } & RestoreFocusProp) {
  return <ModalShell titleId="review-title" className={styles.reviewModal} onClose={onClose} restoreFocus={restoreFocus}><div className={styles.modalHeader}><div><h2 id="review-title">Review the actual change</h2></div><button className={styles.iconButton} onClick={onClose} aria-label="Close"><Icon name="close" /></button></div><div className={styles.reviewBanner}><span className={styles.reviewIcon}><Icon name="diff" /></span><div><strong>One approval authorizes this displayed patch transaction only.</strong><span>{repository?.isGitRepository ? "Git preflight will recheck every base hash and apply without staging." : "Git-backed application is unavailable for this folder; export the patch for external review."}</span></div></div><div className={styles.diffFiles}>{review.files.map((file) => <div key={file.path} className={styles.diffFile}><div className={styles.diffFileHeader}><code>{file.path}</code><span>before / after</span></div><Suspense fallback={<div className={styles.codeLoading}>Loading diff…</div>}><DiffSurface file={file} /></Suspense></div>)}</div><details className={styles.unifiedDetails}><summary>Show canonical unified diff</summary><pre>{review.diff}</pre></details>{review.behaviorRationale.length > 0 && <div className={styles.reviewRationale}><span className={styles.labelObserved}>BEHAVIOR RATIONALE</span>{review.behaviorRationale.map((risk) => <p key={risk}>{risk}</p>)}</div>}{review.risks.length > 0 && <div className={styles.reviewRisks}><span className={styles.labelWarning}>UNRESOLVED RISKS</span>{review.risks.map((risk) => <p key={risk}>! {risk}</p>)}</div>}<label className={styles.reviewCheck}><input type="checkbox" checked={reviewed} onChange={(event) => onReviewed(event.target.checked)} /> I reviewed the complete diff, touched files, behavior rationale, and unresolved risks.</label><div className={styles.modalActions}><button className={styles.secondaryButton} onClick={onClose}>Keep proposal</button><button className={styles.primaryButton} onClick={onApply} disabled={!reviewed || !repository?.isGitRepository}>Apply reviewed patch</button></div>{!repository?.isGitRepository && <p className={styles.modalHint}>This repository is not a Git worktree. CXView keeps the proposal exportable and will not pretend it can safely apply a checked patch here.</p>}</ModalShell>;
}

function CommandDialog({ candidate, onCancel, onApprove, restoreFocus }: { candidate: ValidationCandidate; onCancel: () => void; onApprove: () => void } & RestoreFocusProp) {
  return <ModalShell titleId="command-title" onClose={onCancel} restoreFocus={restoreFocus}><div className={styles.modalHeader}><div><h2 id="command-title">Run a repository check?</h2></div><button className={styles.iconButton} onClick={onCancel} aria-label="Close"><Icon name="close" /></button></div><div className={styles.commandPreview}><code>{candidate.executable} {candidate.args.join(" ")}</code><span>cwd: {candidate.workingDirectory}</span><span>{candidate.networkNote}</span>{candidate.expectedWrites.map((write) => <span key={write}>writes: {write}</span>)}</div><p>Repository scripts can invoke lifecycle hooks and arbitrary code. CXView removes known provider API-key variables but cannot sandbox this command.</p><div className={styles.modalActions}><button className={styles.secondaryButton} onClick={onCancel}>Cancel</button><button className={styles.primaryButton} onClick={onApprove}>Approve one run</button></div></ModalShell>;
}

function ProviderDialog({ provider, hasTask, onClose, onGenerate, restoreFocus }: { provider: ProviderDiagnostic; hasTask: boolean; onClose: () => void; onGenerate: () => void } & RestoreFocusProp) {
  return <ModalShell titleId="provider-title" onClose={onClose} restoreFocus={restoreFocus}><div className={styles.modalHeader}><div><h2 id="provider-title">Optional proposal adapter</h2></div><button className={styles.iconButton} onClick={onClose} aria-label="Close"><Icon name="close" /></button></div><p>{provider.provider} is proposal-only. It cannot apply patches, run validation, or change scanner status.</p><div className={styles.storageSummary}><strong>{provider.installed ? "Detected" : "Not installed"}{provider.version ? ` · ${provider.version}` : ""}</strong><span>Executable: {provider.executable ?? "not found on PATH"}</span><span>Structured output: {provider.schemaOutputSupported ? "advertised" : "not verified"} · read-only flag: {provider.readOnlyFlagSupported ? "advertised" : "not verified"}</span></div><div className={styles.disclosure}><span className={styles.labelScanner}>AUTHENTICATION</span><p>{provider.authentication}</p><span className={styles.labelScanner}>PERMISSIONS</span><p>{provider.permissions}</p><span className={styles.labelScanner}>DATA DESTINATION</span><p>{provider.dataDestination}</p></div><p className={styles.modalHint}>{provider.diagnostic}</p>{provider.integrationEnabled ? <button className={styles.primaryButton} onClick={onGenerate} disabled={!hasTask || !api.isNative}>Generate schema-validated proposal</button> : <p className={styles.modalHint}>Integrated generation is disabled until CXView can enforce the provider boundary. Use “Export task” and “Import proposal” for an explicit external handoff.</p>}<div className={styles.modalActions}><button className={styles.secondaryButton} onClick={onClose}>Close</button></div></ModalShell>;
}

function ValidationDrawer({ candidates, runs, loading, error, suspended, onClose, onRetry, onRun, restoreFocus }: { candidates: ValidationCandidate[]; runs: ValidationRun[]; loading: boolean; error?: string; suspended: boolean; onClose: () => void; onRetry: () => void; onRun: (candidate: ValidationCandidate, trigger: HTMLButtonElement) => void } & RestoreFocusProp) {
  const dialogRef = useDialogFocus(onClose, restoreFocus);
  return <><div className={styles.drawerBackdrop} aria-hidden="true" onMouseDown={suspended ? undefined : onClose} /><section ref={dialogRef} className={styles.validationDrawer} role={suspended ? undefined : "dialog"} aria-modal={suspended ? undefined : "true"} aria-hidden={suspended ? "true" : undefined} inert={suspended ? true : undefined} aria-labelledby="validation-title" aria-describedby="validation-description" tabIndex={-1}><div className={styles.validationHeader}><div><h2 id="validation-title">Checks from the actual manifest</h2><p id="validation-description">Validation candidates are discovered from the bound repository. Each run needs a separate approval.</p></div><button className={styles.iconButton} onClick={onClose} aria-label="Close validation drawer"><Icon name="close" /></button></div><div className={styles.validationGrid}><div><p className={styles.modalHint}>No command was invented. Each candidate below came from package.json and may run lifecycle hooks or use the network.</p>{loading ? <div className={styles.validationStatus} role="status"><strong>Discovering repository checks…</strong><span>Reading the actual manifest and task-bound validation history.</span></div> : error ? <div className={`${styles.validationStatus} ${styles.validationStatusError}`} role="alert"><strong>Validation evidence could not be loaded</strong><span>{error}</span><button className={styles.secondaryButton} onClick={onRetry}>Try again</button></div> : <>{candidates.map((candidate) => <div className={styles.validationCandidate} key={candidate.id}><div><strong>{candidate.id.replace("script-", "")}</strong><code>{candidate.executable} {candidate.args.join(" ")}</code></div><button className={styles.secondaryButton} onClick={(event) => onRun(candidate, event.currentTarget)}>Approve + run</button></div>)}{candidates.length === 0 && <div className={styles.emptyList}><strong>No check candidates</strong><span>There is no supported script in this package root.</span></div>}</>}</div><div><div className={styles.sectionTitle}>Recorded runs <span>bound to task snapshot</span></div>{loading ? <div className={styles.emptyList}><strong>Loading recorded runs</strong><span>Only evidence bound to this task snapshot will appear here.</span></div> : runs.map((run) => <div className={styles.runRow} key={run.id}><span className={`${styles.runStatus} ${run.status === "passed" ? styles.runPassed : run.status === "failed" ? styles.runFailed : styles.runUnknown}`}>{run.status}</span><div><strong>{run.candidateId}</strong><span>{run.durationMs} ms · exit {run.exitCode ?? "—"}</span><p>{run.note}</p>{run.stderr && <pre>{run.stderr}</pre>}{run.stdout && <pre>{run.stdout}</pre>}</div></div>)}{!loading && runs.length === 0 && <div className={styles.emptyList}><strong>No validation evidence yet</strong><span>Run a baseline or post-change check after reviewing its command.</span></div>}</div></div></section></>;
}

function UnselectedState({ hasRepository, onBind }: { hasRepository: boolean; onBind: () => void }) { return <div className={styles.unselected}><div className={styles.unselectedIcon}><Icon name="focus" /></div><h2>Choose a finding</h2><p>{hasRepository ? "The center pane will show scanner nodes beside current read-only source." : "Bind a repository to unlock current-code evidence. Report evidence remains visible without one."}</p>{!hasRepository && <button className={styles.secondaryButton} onClick={onBind}>Bind repository</button>}</div>; }

function stateClass(state: string) { return styles[`match${state}`] ?? styles.matchunavailable; }
function taskStateClass(state: TaskState) { return styles[`task${state}`] ?? styles.taskinvestigating; }
function errorMessage(error: unknown): string { return error instanceof Error ? error.message : typeof error === "string" ? error : "CXView could not complete that operation."; }
function emptyProvider(): ProviderDiagnostic { return { provider: "Codex CLI", installed: false, schemaOutputSupported: false, readOnlyFlagSupported: false, integrationEnabled: false, authentication: "Not checked", permissions: "Not checked", dataDestination: "No automatic outbound requests", diagnostic: "Provider diagnostic unavailable in browser preview." }; }
function preventDefault(event: Event) { event.preventDefault(); }
function shortPath(path: string) { const parts = path.replaceAll("\\", "/").split("/"); return parts.length > 3 ? `…/${parts.slice(-2).join("/")}` : path; }
function capitalize(value: string) { return value.slice(0, 1).toUpperCase() + value.slice(1); }
function severityClass(severity: string) { return `${styles[`severity${severity}`] ?? styles.severityUnknown}`; }
function readinessLabel(readiness: FindingSummary["evidenceReadiness"]) { return readiness === "ready" ? "evidence ready" : readiness === "partial" ? "partial evidence" : readiness === "ambiguous" ? "ambiguous" : "evidence missing"; }
function viewLabel(value: string) { return value.replaceAll("-", " ").replace(/\b\w/g, (letter) => letter.toUpperCase()); }
function clearFilter(filter: string, setSeverity: (value: string) => void, setEngine: (value: string) => void, setStatus: (value: string) => void, setView: (value: string) => void) { if (filter.startsWith("severity:")) setSeverity(""); else if (filter.startsWith("engine:")) setEngine(""); else if (filter.startsWith("status:")) setStatus(""); else setView(""); }
function formatBytes(bytes: number) { if (bytes < 1024) return `${bytes} B`; if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`; return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`; }
function clamp(value: number, minimum: number, maximum: number) { return Math.max(minimum, Math.min(maximum, value)); }

export default App;
