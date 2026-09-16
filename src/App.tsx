import { useCallback, useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { CodeSurface, DiffSurface } from "./components/CodeSurface";
import * as api from "./lib/api";
import type { AppSnapshot, ComparisonSummary, FindingSummary, ImportDiagnostics, ImportResult, InvestigationBundle, PatchReview, Profile, ProviderDiagnostic, RawInspection, RemediationTask, RepositoryContext, ReportSummary, StorageInfo, TaskState, ValidationCandidate, ValidationRun } from "./lib/types";
import styles from "./styles/App.module.css";

type Message = { tone: "info" | "success" | "error"; text: string } | null;
type Modal = "proposal" | "command" | "provider" | null;
type ResizeSide = "left" | "right";

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
  const [loading, setLoading] = useState(false);

  const notify = useCallback((tone: "info" | "success" | "error", text: string) => {
    setMessage({ tone, text });
  }, []);

  const refresh = useCallback(async () => {
    try {
      const next = await api.loadSnapshot();
      setSnapshot(next);
      setReport(next.report);
      setDiagnostics(next.diagnostics);
      setProfile(next.profile);
      setRepository(next.repository);
      setFindings(next.findings);
      setProvider(next.provider);
      if (next.profile?.uiState.selectedFindingId) setSelectedId(next.profile.uiState.selectedFindingId);
      if (next.profile?.uiState.search) setSearch(next.profile.uiState.search);
      if (next.profile?.uiState.theme === "light" || next.profile?.uiState.theme === "dark") setTheme(next.profile.uiState.theme);
      if (next.profile?.uiState.codeFontSize) setFontSize(next.profile.uiState.codeFontSize);
      if (typeof next.profile?.uiState.leftWidth === "number") setLeftWidth(clamp(next.profile.uiState.leftWidth, 270, 520));
      if (typeof next.profile?.uiState.rightWidth === "number") setRightWidth(clamp(next.profile.uiState.rightWidth, 330, 520));
      const selectedTask = next.tasks.find((item) => item.findingIds.includes(next.profile?.uiState.selectedFindingId ?? ""));
      if (selectedTask) setTask(selectedTask);
    } catch (error) {
      notify("error", errorMessage(error));
    }
  }, [notify]);

  useEffect(() => { void refresh(); }, [refresh]);

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
  }, [notify]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    if (api.isNative) {
      void getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === "drop") void acceptDroppedPaths(event.payload.paths);
      }).then((dispose) => { if (disposed) dispose(); else unlisten = dispose; });
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
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement || !findings.length) return;
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
    if (!report) return;
    const timer = window.setTimeout(() => {
      void api.listFindings({ reportId: report.id, search, severity: severityFilter || undefined, engine: engineFilter || undefined, status: statusFilter || undefined, savedView: savedView || undefined, page: 0, pageSize: 200 }).then(setFindings).catch((error) => notify("error", errorMessage(error)));
    }, 180);
    return () => window.clearTimeout(timer);
  }, [report, search, severityFilter, engineFilter, statusFilter, savedView, notify]);

  useEffect(() => {
    if (!selectedId) return;
    setInvestigation(undefined);
    void api.investigation(selectedId).then((bundle) => {
      setInvestigation(bundle);
      setRepoPath(repository?.path ?? profile?.repositoryPath ?? "");
    }).catch((error) => notify("error", errorMessage(error)));
  }, [selectedId, notify, profile?.repositoryPath, repository?.path]);

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

  const applyImport = (result: ImportResult) => {
    setReport(result.report);
    setDiagnostics(result.diagnostics);
    setFindings(result.findings);
    setComparison(result.comparison);
    setSelectedId(result.findings[0]?.id);
    setInvestigation(undefined);
    setTask(undefined);
    setReview(undefined);
    setShowBind(!repository);
  };

  const openReport = async () => {
    const path = await api.chooseReport();
    if (!path) { if (!api.isNative) notify("info", "Run `pnpm tauri dev` to use native file selection."); return; }
    setLoading(true);
    try { applyImport(await api.importReport(path)); notify("success", "Report imported as an immutable snapshot; inspect diagnostics before trusting the parsed count."); } catch (error) { notify("error", errorMessage(error)); } finally { setLoading(false); }
  };

  const chooseRepo = async () => {
    const path = await api.chooseRepository();
    if (!path) { if (!api.isNative) notify("info", "Run `pnpm tauri dev` to use native folder selection."); return; }
    setRepoPath(path); setShowBind(true);
  };

  const bindRepo = async () => {
    if (!report || !repoPath) return;
    setLoading(true);
    try {
      const context = await api.bindRepository({ reportId: report.id, repositoryPath: repoPath, scanPrefix: scanPrefix || undefined, repositoryPrefix: repositoryPrefix || undefined });
      setRepository(context); setShowBind(false); await refresh(); notify("success", `Bound ${context.canonicalPath}. ${context.isGitRepository ? "Git-aware review and apply are available." : "Git-backed apply is unavailable; export remains available."}`);
      if (selectedId) setInvestigation(await api.investigation(selectedId));
    } catch (error) { notify("error", errorMessage(error)); } finally { setLoading(false); }
  };

  const ensureTask = async (): Promise<RemediationTask | undefined> => {
    if (!selectedId) return undefined;
    const existing = snapshot.tasks.find((item) => item.findingIds.includes(selectedId)) ?? (task?.findingIds.includes(selectedId) ? task : undefined);
    if (existing) { setTask(existing); return existing; }
    try { const created = await api.createTask(selectedId); setTask(created); setSnapshot((previous) => ({ ...previous, tasks: [created, ...previous.tasks] })); return created; } catch (error) { notify("error", errorMessage(error)); return undefined; }
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
    const current = task ?? await ensureTask();
    if (!current) return;
    try { const next = await api.reviewPatch(current.id); setReview(next); setReviewed(false); notify("info", "Review the complete patch. Applying remains a separate approval and will recheck every base hash."); } catch (error) { notify("error", errorMessage(error)); }
  };

  const startProposal = async () => {
    const current = task ?? await ensureTask();
    if (current) setModal("proposal");
  };

  const runProviderProposal = async () => {
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
    const current = task ?? await ensureTask();
    if (!current) return;
    const path = await api.chooseProposal();
    if (!path) return;
    try { const next = await api.importProposal(current.id, path); setTask(await api.getTask(current.id)); setReview(next); setReviewed(false); notify("success", "External proposal imported as a bound, un-applied proposal. Review it before any write."); } catch (error) { notify("error", errorMessage(error)); }
  };

  const exportTask = async () => {
    const current = task ?? await ensureTask();
    if (!current) return;
    const path = await api.chooseExportPath(`cxview-${current.id}.json`);
    if (!path) return;
    try { await api.exportTask(current.id, path); notify("success", `Task bundle exported to ${path}. Review proprietary source and scanner evidence before sharing.`); } catch (error) { notify("error", errorMessage(error)); }
  };

  const copyPrompt = async () => {
    if (!selectedId) return;
    try { const prompt = await api.investigationPrompt(selectedId); await navigator.clipboard.writeText(prompt); notify("success", "Investigation prompt copied. It contains bounded evidence and a proposal contract, not authorization to write."); } catch (error) { notify("error", errorMessage(error)); }
  };

  const inspectRaw = async (locator: string) => {
    if (!report) return;
    try { setRawInspection(await api.readRawLocator(report.id, locator)); } catch (error) { notify("error", errorMessage(error)); }
  };

  const openStorage = async () => {
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
    const current = task ?? await ensureTask();
    if (!current) return;
    try { setValidationCandidates(await api.discoverValidation(current.id)); setValidationRuns(await api.validationHistory(current.id)); setValidationOpen(true); } catch (error) { notify("error", errorMessage(error)); setValidationOpen(true); }
  };

  const confirmCommand = async () => {
    if (!task || !commandCandidate) return;
    setModal(null);
    try { const run = await api.runValidation({ taskId: task.id, candidateId: commandCandidate.id, approved: true }); setValidationRuns((previous) => [run, ...previous]); setTask(await api.getTask(task.id)); notify(run.status === "passed" ? "success" : "error", run.note); } catch (error) { notify("error", errorMessage(error)); }
  };

  const selected = findings.find((finding) => finding.id === selectedId);
  const activeFilters = [severityFilter && `severity: ${severityFilter}`, engineFilter && `engine: ${engineFilter}`, statusFilter && `status: ${statusFilter}`, savedView && `view: ${viewLabel(savedView)}`].filter(Boolean) as string[];

  return (
    <main className={styles.app} data-theme={theme}>
      <header className={styles.topbar}>
        <div className={styles.brandBlock}><div className={styles.logoMark}>CX</div><div><div className={styles.brand}>CXView</div><div className={styles.eyebrow}>LOCAL REMEDIATION WORKBENCH</div></div></div>
        <div className={styles.workspaceCrumb}>{report ? report.sourceName : "Open report + select repository"}<span className={styles.dot}>·</span>{repository ? shortPath(repository.canonicalPath) : "No repository bound"}</div>
        <div className={styles.topActions}><button className={styles.ghostButton} onClick={() => setTheme(theme === "dark" ? "light" : "dark")} aria-label="Toggle light and dark theme">{theme === "dark" ? "Light" : "Dark"}</button><label className={styles.fontControl}>Code <input type="range" min="11" max="18" value={fontSize} onChange={(event) => setFontSize(Number(event.target.value))} aria-label="Code text size" /></label><button className={styles.ghostButton} onClick={() => void openStorage()}>Storage</button><button className={styles.ghostButton} onClick={() => void refresh()}>Refresh</button></div>
      </header>

      {message && <div className={`${styles.notice} ${styles[`notice${capitalize(message.tone)}`]}`} role="status"><span>{message.text}</span><button className={styles.noticeClose} onClick={() => setMessage(null)} aria-label="Dismiss message">×</button></div>}

      {!report ? <EmptyState onOpen={openReport} loading={loading} /> : <>
        <div className={styles.subbar}><div className={styles.reportMeta}><span className={styles.pill}>Snapshot {report.sha256.slice(0, 12)}</span><span>{report.findingCount} parsed instances</span><span>{report.adapterId} v{report.adapterVersion}</span>{report.metadata.branch && <span>branch {report.metadata.branch}</span>}</div><div className={styles.subActions}><button className={styles.secondaryButton} onClick={openReport}>Import later report</button><button className={styles.secondaryButton} onClick={chooseRepo}>Bind repository</button></div></div>
        <div ref={workspaceRef} className={styles.workspace} style={{ gridTemplateColumns: `${leftWidth}px 8px minmax(380px, 1fr) 8px ${rightWidth}px` }}>
          <aside className={`${styles.pane} ${styles.queuePane}`} aria-label="Finding work queue">
            <div className={styles.paneHeader}><div><div className={styles.paneKicker}>WORK QUEUE</div><h2>Findings</h2></div><span className={styles.countBadge}>{findings.length}{findings.length === 200 ? "+" : ""}</span></div>
            <div className={styles.searchWrap}><span className={styles.searchIcon}>⌕</span><input ref={searchRef} value={search} onChange={(event) => setSearch(event.target.value)} placeholder="Search rule, file, package…" aria-label="Search findings" /><kbd>⌘K</kbd></div>
            <div className={styles.filterGrid}><select value={severityFilter} onChange={(event) => setSeverityFilter(event.target.value)} aria-label="Filter severity"><option value="">All severity</option><option>Critical</option><option>High</option><option>Medium</option><option>Low</option><option>Informational</option><option>Unknown</option></select><select value={engineFilter} onChange={(event) => setEngineFilter(event.target.value)} aria-label="Filter scanner engine"><option value="">All engines</option><option>SAST</option><option>SCA</option><option>IaC</option><option>Unknown</option></select></div>
            <select className={styles.savedView} value={savedView} onChange={(event) => setSavedView(event.target.value)} aria-label="Saved finding view"><option value="">All findings</option><option value="needs-investigation">Needs investigation</option><option value="ready-to-review">Ready to review</option><option value="patched-awaiting-validation">Patched awaiting validation</option><option value="awaiting-rescan">Awaiting rescan</option></select>
            {activeFilters.length > 0 && <div className={styles.activeFilters}><span>Active</span>{activeFilters.map((filter) => <button key={filter} onClick={() => clearFilter(filter, setSeverityFilter, setEngineFilter, setStatusFilter, setSavedView)}>{filter} ×</button>)}</div>}
            <div className={styles.findingList}>{findings.map((finding) => <FindingRow key={finding.id} finding={finding} selected={finding.id === selectedId} onSelect={() => setSelectedId(finding.id)} />)}{findings.length === 0 && <div className={styles.emptyList}><strong>No matching findings</strong><span>Clear a filter or inspect import diagnostics. A zero result is not treated as a failed import.</span></div>}</div>
            <div className={styles.queueFooter}><span>Keyboard: ↑↓ / J K</span><span>{diagnostics?.unsupportedRecords.length ?? 0} unsupported retained</span></div>
          </aside>

          <div className={styles.paneDivider} role="separator" aria-orientation="vertical" aria-label="Resize findings pane" tabIndex={0} onPointerDown={(event) => beginResize("left", event)} onKeyDown={(event) => keyboardResize("left", event)} />

          <section className={`${styles.pane} ${styles.evidencePane}`} aria-label="Evidence and code">
            {!investigation || !selected ? <UnselectedState hasRepository={Boolean(repository)} onBind={chooseRepo} /> : <EvidencePane investigation={investigation} fontSize={fontSize} />}
          </section>

          <div className={styles.paneDivider} role="separator" aria-orientation="vertical" aria-label="Resize remediation pane" tabIndex={0} onPointerDown={(event) => beginResize("right", event)} onKeyDown={(event) => keyboardResize("right", event)} />

          <aside className={`${styles.pane} ${styles.remediationPane}`} aria-label="Remediation workspace">
            {!investigation || !selected ? <div className={styles.remediationEmpty}><div className={styles.bigIcon}>↗</div><h2>Investigation starts here</h2><p>Select a finding to see the exact scanner claim, current-code observations, unknowns, and an actionable playbook.</p></div> : <RemediationPane investigation={investigation} task={task} provider={provider} onInvestigate={() => void investigate()} onSuggest={() => void suggestFix()} onProposal={() => void startProposal()} onImport={importProposal} onReview={() => void reviewChanges()} onExport={() => void exportTask()} onCopy={() => void copyPrompt()} onRunChecks={() => void openValidation()} onUndo={() => void undoPatch()} onProvider={() => setModal("provider")} onInspectRaw={(locator) => void inspectRaw(locator)} onNote={(note) => { void api.saveNote(investigation.finding.id, note).catch((error) => notify("error", errorMessage(error))); }} />}
          </aside>
        </div>
        {diagnostics && <DiagnosticsStrip diagnostics={diagnostics} report={report} comparison={comparison} />}
        {showBind && <BindRepositoryDialog report={report} path={repoPath} scanPrefix={scanPrefix} repositoryPrefix={repositoryPrefix} onPathChange={setRepoPath} onScanPrefixChange={setScanPrefix} onRepositoryPrefixChange={setRepositoryPrefix} onBind={() => void bindRepo()} onCancel={() => setShowBind(false)} />}
        {review && <ReviewDialog review={review} reviewed={reviewed} onReviewed={setReviewed} onApply={() => void applyPatch()} onClose={() => setReview(undefined)} repository={repository} />}
        {modal === "proposal" && task && investigation && <ProposalDialog task={task} investigation={investigation} onClose={() => setModal(null)} onCreated={(next) => { setReview(next); setReviewed(false); void api.getTask(task.id).then(setTask).catch(() => undefined); setModal(null); notify("success", "Proposal buffer converted into a real, precondition-checked diff. Review it before applying."); }} />}
        {modal === "command" && commandCandidate && task && <CommandDialog candidate={commandCandidate} onCancel={() => setModal(null)} onApprove={() => void confirmCommand()} />}
        {modal === "provider" && <ProviderDialog provider={provider} hasTask={Boolean(task)} onClose={() => setModal(null)} onGenerate={() => void runProviderProposal()} />}
        {validationOpen && <ValidationDrawer candidates={validationCandidates} runs={validationRuns} onClose={() => setValidationOpen(false)} onRun={(candidate) => { setCommandCandidate(candidate); setModal("command"); }} />}
        {rawInspection && <RawInspectionDialog inspection={rawInspection} onClose={() => setRawInspection(undefined)} />}
        {storageOpen && storage && <StorageDialog profile={profile} info={storage} onClose={() => setStorageOpen(false)} onDelete={(confirmation) => void deleteWorkspace(confirmation)} />}
      </>}
    </main>
  );
}

function EmptyState({ onOpen, loading }: { onOpen: () => void; loading: boolean }) {
  return <section className={styles.emptyState}><div className={styles.emptyOrb}><span>⊹</span></div><div className={styles.paneKicker}>CXONE JSON → REVIEWED CODE CHANGE</div><h1>Evidence before edits.</h1><p>Open a supported Checkmarx One JSON report, bind the repository it came from, and carry one finding through investigation, review, application, and local validation.</p><div className={styles.emptyActions}><button className={styles.primaryButton} onClick={onOpen} disabled={loading}>{loading ? "Importing…" : "Open report"}</button><span>or drag a JSON report onto this window</span></div><div className={styles.capabilityRow}><span>Offline import + guidance</span><span>Native path checks</span><span>Reviewed patch journal</span><span>Local validation evidence</span></div></section>;
}

function FindingRow({ finding, selected, onSelect }: { finding: FindingSummary; selected: boolean; onSelect: () => void }) {
  return <button className={`${styles.findingRow} ${selected ? styles.findingSelected : ""}`} onClick={onSelect} aria-current={selected ? "true" : undefined}><div className={styles.findingTop}><span className={`${styles.severity} ${severityClass(finding.severity)}`}>{finding.severity}</span><span className={styles.engineLabel}>{finding.category.toUpperCase()}</span><span className={styles.readiness}>{readinessLabel(finding.evidenceReadiness)}</span></div><strong>{finding.title}</strong><span className={styles.findingLocation}>{finding.filePath ?? finding.packageName ?? "No location supplied"}{finding.lineStart ? `:${finding.lineStart}` : ""}</span><div className={styles.findingBottom}><span>{finding.rule ?? "Rule unknown"}</span><span>{taskLabels[finding.localTaskState]}</span></div></button>;
}

function EvidencePane({ investigation, fontSize }: { investigation: InvestigationBundle; fontSize: number }) {
  const { finding, context } = investigation;
  const [jumpToken, setJumpToken] = useState(0);
  const focusLine = context.currentRangeStart && finding.lineStart ? Math.max(1, finding.lineStart - context.currentRangeStart + 1) : undefined;
  return <div className={styles.paneContent}><div className={styles.evidenceHeader}><div><div className={styles.paneKicker}>EVIDENCE + CODE</div><h2>{finding.filePath ?? finding.packageName ?? finding.resource ?? "No location supplied"}</h2><div className={styles.locationLine}>{finding.lineStart ? `reported line ${finding.lineStart}${finding.lineEnd && finding.lineEnd !== finding.lineStart ? `–${finding.lineEnd}` : ""}` : "location not supplied"}<span className={`${styles.stateBadge} ${stateClass(context.matchState)}`}>{matchLabels[context.matchState] ?? context.matchState}</span></div></div><button className={styles.ghostButton} onClick={() => setJumpToken((value) => value + 1)} disabled={!context.currentSource || !focusLine} title="Source navigation is native and read-only">Jump to reported location</button></div><div className={styles.evidenceTabs}><span className={styles.activeTab}>Current source</span><span>Reported flow · {context.reportedNodes.length} step{context.reportedNodes.length === 1 ? "" : "s"}</span><span>Related files · {context.relatedFiles.length}</span></div>{context.currentSource ? <div className={styles.codeFrame}><div className={styles.codeFrameBar}><span>{context.relativePath ?? finding.filePath}</span><span>SHA {context.sourceHash?.slice(0, 12) ?? "unknown"}</span></div><CodeSurface content={context.currentSource} filePath={context.relativePath} fontSize={fontSize} focusLine={focusLine} focusToken={jumpToken} /></div> : <div className={styles.noSource}><span className={styles.warningGlyph}>!</span><div><strong>Current source is not available for editing</strong><p>{context.observedLocal.map((statement) => statement.text).join(" ")}</p>{context.reportedSnippet && <pre>{context.reportedSnippet}</pre>}</div></div>}{context.reportedNodes.length > 0 && <div className={styles.flowPanel}><div className={styles.sectionTitle}>Reported source / sink / flow steps <span>scanner evidence</span></div><div className={styles.flowList}>{context.reportedNodes.map((node) => <div className={styles.flowRow} key={`${node.locator}-${node.order}`}><span className={styles.flowNumber}>{node.order + 1}</span><div><strong>{node.role ?? node.sourceSink ?? "reported node"}</strong><span>{node.path ?? "path not supplied"}{node.lineStart ? `:${node.lineStart}` : ""}</span>{node.snippet && <code>{node.snippet}</code>}</div></div>)}</div></div>}{finding.category === "sca" && context.sca && <DependencyCard dependency={context.sca} />}</div>;
}

function DependencyCard({ dependency }: { dependency: NonNullable<InvestigationBundle["context"]["sca"]> }) {
  return <div className={styles.dependencyCard}><div className={styles.sectionTitle}>Dependency context <span>observed locally</span></div><div className={styles.dependencySummary}><span className={styles.packageIcon}>pkg</span><div><strong>{dependency.packageName ?? "package identity unknown"}</strong><span>scan version {dependency.scanVersion ?? "unknown"} · {dependency.directDependency === true ? "direct" : dependency.directDependency === false ? "transitive" : "ownership unknown"}</span></div></div>{dependency.resolvedInstances.length > 0 && <div className={styles.instanceList}>{dependency.resolvedInstances.map((instance) => <div key={`${instance.installPath}-${instance.version}`}><code>{instance.installPath}</code><span>{instance.version ?? "version unknown"}</span></div>)}</div>}{dependency.ownershipPaths.length > 0 && <div className={styles.mutedList}>{dependency.ownershipPaths.slice(0, 8).map((path) => <span key={path}>{path}</span>)}</div>}{dependency.notes.map((note) => <div className={styles.inlineWarning} key={note}>! {note}</div>)}{dependency.inspectionCommands.length > 0 && <details className={styles.commandDetails}><summary>Reviewed inspection commands</summary>{dependency.inspectionCommands.map((command) => <code key={command}>{command}</code>)}</details>}</div>;
}

function RemediationPane({ investigation, task, provider, onInvestigate, onSuggest, onProposal, onImport, onReview, onExport, onCopy, onRunChecks, onUndo, onProvider, onInspectRaw, onNote }: { investigation: InvestigationBundle; task?: RemediationTask; provider: ProviderDiagnostic; onInvestigate: () => void; onSuggest: () => void; onProposal: () => void; onImport: () => void; onReview: () => void; onExport: () => void; onCopy: () => void; onRunChecks: () => void; onUndo: () => void; onProvider: () => void; onInspectRaw: (locator: string) => void; onNote: (note: string) => void }) {
  const { finding, context, playbook } = investigation;
  const [note, setNote] = useState(investigation.localNote ?? "");
  useEffect(() => setNote(investigation.localNote ?? ""), [investigation.localNote, finding.id]);
  return <div className={styles.paneContent}><div className={styles.remediationHeader}><div><div className={styles.paneKicker}>REMEDIATION</div><h2>Make this finding explainable</h2></div>{task && <span className={`${styles.stateBadge} ${taskStateClass(task.state)}`}>{taskLabels[task.state]}</span>}</div><div className={styles.actionGrid}><button className={styles.primaryButton} onClick={onInvestigate}>Investigate</button><button className={styles.secondaryButton} onClick={onSuggest}>Suggest fix</button><button className={styles.secondaryButton} onClick={onReview} disabled={!task?.proposal}>Review changes</button><button className={styles.secondaryButton} onClick={onRunChecks} disabled={!task}>Run checks</button></div><div className={styles.disclosure}><span className={styles.labelScanner}>SCANNER EVIDENCE</span><strong>What was reported</strong><p>{finding.description ?? "No description was supplied by the report."}</p><div className={styles.factGrid}><span>Engine <b>{finding.engine}</b></span><span>Original severity <b>{finding.originalSeverity ?? "Unknown"}</b></span><span>Result status <b>{finding.resultStatus ?? "Unknown"}</b></span><span>Triage state <b>{finding.triageState ?? "Unknown"}</b></span><span>Rule/query <b>{finding.rule ?? "Unknown"}</b></span><span>Raw locator <b>{finding.rawLocator} <button className={styles.rawInspectButton} onClick={() => onInspectRaw(finding.rawLocator)}>Inspect JSON</button></b></span></div></div><div className={styles.disclosure}><span className={styles.labelObserved}>OBSERVED LOCALLY</span><strong>What current code shows</strong>{context.observedLocal.map((statement) => <p key={`${statement.text}-${statement.locator}`}>{statement.text}</p>)}{context.syntax && <div className={styles.syntaxMeta}><span>{context.syntax.parser} {context.syntax.parserVersion}</span><span>{context.syntax.syntaxErrorCount} syntax error{context.syntax.syntaxErrorCount === 1 ? "" : "s"}</span></div>}</div><div className={styles.disclosure}><span className={styles.labelUnknown}>NOT ESTABLISHED</span><strong>Unknowns and drift</strong><ul>{context.unknowns.length > 0 ? context.unknowns.map((unknown) => <li key={unknown}>{unknown}</li>) : <li>No additional unknowns recorded; absence of unknowns is not proof of safety.</li>}</ul></div><div className={styles.playbook}><div className={styles.playbookHeading}><div><span className={styles.paneKicker}>OFFLINE PLAYBOOK · v1</span><h3>{playbook.title}</h3></div><span>reviewed {playbook.lastReviewed}</span></div><p className={styles.playbookNote}>{playbook.note}</p><PlaybookSection title="Ask first" values={playbook.investigationQuestions} /><PlaybookSection title="Preferred repair" values={playbook.preferredRepairs} /><PlaybookSection title="Preserve + test" values={[...playbook.behaviorPreservation, ...playbook.regressionTests]} /><PlaybookSection title="Reject cosmetic fixes" values={playbook.contraindications} warning />{playbook.references.map((reference) => <a className={styles.reference} key={reference.url} href={reference.url} target="_blank" rel="noreferrer">↗ {reference.label}</a>)}</div><div className={styles.noteBlock}><div className={styles.sectionTitle}>Local assessment <span>user supplied</span></div><textarea value={note} onChange={(event) => setNote(event.target.value)} onBlur={() => onNote(note)} placeholder="Record a false-positive rationale or investigation note…" maxLength={32768} /></div><div className={styles.proposalTools}><div className={styles.sectionTitle}>Proposal exchange <span>never auto-applies</span></div><div className={styles.toolButtons}><button className={styles.primaryButton} onClick={onProposal}>Edit proposal buffer</button><button className={styles.secondaryButton} onClick={onImport}>Import proposal</button><button className={styles.secondaryButton} onClick={onExport}>Export task</button><button className={styles.secondaryButton} onClick={onCopy}>Copy investigation prompt</button></div>{task?.proposal && <div className={styles.proposalState}><span className={styles.successDot} /> {task.proposal.source} proposal bound to snapshot · {task.proposal.edits.length} edit{task.proposal.edits.length === 1 ? "" : "s"}<button className={styles.linkButton} onClick={onReview}>Open diff</button></div>}</div>{task?.patchId && <div className={styles.appliedCard}><div><span className={styles.labelObserved}>PATCH JOURNAL</span><strong>{task.state === "awaitingrescan" ? "Applied and locally validated" : "Applied"}</strong><p>Scanner status is separate. CXView will not mark this finding fixed until a comparable later export says so.</p></div><button className={styles.secondaryButton} onClick={onUndo}>Undo this CXView patch</button></div>}<button className={styles.providerCard} onClick={onProvider}><span className={provider.integrationEnabled ? styles.providerLive : styles.providerOff}>{provider.integrationEnabled ? "●" : "○"}</span><div><strong>Optional Codex proposal adapter</strong><span>{provider.diagnostic}</span></div><em>{provider.integrationEnabled ? "available" : "disabled"}</em></button></div>;
}

function PlaybookSection({ title, values, warning = false }: { title: string; values: string[]; warning?: boolean }) { return <div className={styles.playbookSection}><span className={warning ? styles.labelWarning : styles.labelMuted}>{title}</span>{values.map((value) => <p className={warning ? styles.warningText : ""} key={value}>{value}</p>)}</div>; }

function DiagnosticsStrip({ diagnostics, report, comparison }: { diagnostics: ImportDiagnostics; report: ReportSummary; comparison?: ComparisonSummary }) {
  return <section className={styles.diagnostics}><div className={styles.diagnosticsTop}><div><span className={styles.paneKicker}>IMPORT DIAGNOSTICS</span><strong>{diagnostics.format}</strong><span>adapter {diagnostics.adapterId} v{diagnostics.adapterVersion} · {report.sha256.slice(0, 16)}</span></div><div className={styles.diagnosticCounts}><span><b>{diagnostics.parsedByEngine.sast}</b> SAST</span><span><b>{diagnostics.parsedByEngine.sca}</b> SCA</span><span><b>{diagnostics.parsedByEngine.iac}</b> IaC</span><span><b>{diagnostics.unsupportedRecords.length}</b> unsupported</span><span><b>{diagnostics.malformedRecords.length}</b> malformed</span></div></div>{(diagnostics.countMismatches.length > 0 || diagnostics.warnings.length > 0 || diagnostics.evidenceNotes.length > 0) && <details><summary>Evidence and compatibility notes</summary><div className={styles.diagnosticGrid}>{diagnostics.evidenceNotes.map((note) => <p key={note}>{note}</p>)}{diagnostics.warnings.map((warning) => <p key={warning} className={styles.warningText}>{warning}</p>)}{diagnostics.countMismatches.map((mismatch) => <p key={mismatch.locator} className={styles.warningText}>Count mismatch at {mismatch.locator}: declared {mismatch.declared}, parsed {mismatch.parsed}.</p>)}</div></details>}{diagnostics.unsupportedRecords.length > 0 && <details><summary>Unsupported records retained for raw inspection ({diagnostics.unsupportedRecords.length})</summary><div className={styles.rawList}>{diagnostics.unsupportedRecords.slice(0, 20).map((record) => <div key={record.locator}><code>{record.locator}</code><span>{record.reason}</span><pre>{record.preview}</pre></div>)}</div></details>}{comparison && <div className={styles.comparisonBar}><span className={comparison.provenanceComparable ? styles.successDot : styles.warningGlyph}>{comparison.provenanceComparable ? "✓" : "!"}</span><strong>Later report comparison</strong><span>{comparison.stillObserved} still observed · {comparison.absentUnderComparableScope} absent under comparable scope · {comparison.incomparable} incomparable</span><em>{comparison.provenanceReason}</em></div>}</section>;
}

function RawInspectionDialog({ inspection, onClose }: { inspection: RawInspection; onClose: () => void }) {
  return <div className={styles.modalBackdrop}><section className={styles.modal} role="dialog" aria-modal="true" aria-labelledby="raw-title"><div className={styles.modalHeader}><div><span className={styles.paneKicker}>IMMUTABLE REPORT SNAPSHOT</span><h2 id="raw-title">Raw JSON inspection</h2></div><button className={styles.iconButton} onClick={onClose} aria-label="Close">×</button></div><p><code>{inspection.locator}</code> from the retained imported snapshot. This is report evidence, not a current-code observation.</p><pre className={styles.rawInspection}>{JSON.stringify(inspection.value, null, 2)}</pre><div className={styles.modalActions}><button className={styles.secondaryButton} onClick={onClose}>Close</button></div></section></div>;
}

function StorageDialog({ profile, info, onClose, onDelete }: { profile?: Profile; info: StorageInfo; onClose: () => void; onDelete: (confirmation: string) => void }) {
  const [confirmation, setConfirmation] = useState("");
  const expected = profile ? `DELETE ${profile.id}` : "";
  return <div className={styles.modalBackdrop}><section className={styles.modal} role="dialog" aria-modal="true" aria-labelledby="storage-title"><div className={styles.modalHeader}><div><span className={styles.paneKicker}>LOCAL APP DATA</span><h2 id="storage-title">Storage and retention</h2></div><button className={styles.iconButton} onClick={onClose} aria-label="Close">×</button></div><div className={styles.storageSummary}><strong>{formatBytes(info.bytes)}</strong><span>{info.retainedObjects.length} retained app-data objects</span><code>{info.root}</code></div><p>{info.note} Reports, snapshots, diffs, and validation output can contain proprietary code or secrets; review exports before sharing.</p>{profile ? <><p>This deletes the active workspace profile <code>{profile.id}</code>, its linked report snapshots, tasks, notes, patch journals, and validation history. It does not delete or reset repository files.</p><label>Type <code>{expected}</code> to confirm<input autoFocus value={confirmation} onChange={(event) => setConfirmation(event.target.value)} /></label><div className={styles.modalActions}><button className={styles.secondaryButton} onClick={onClose}>Cancel</button><button className={styles.dangerButton} disabled={confirmation !== expected} onClick={() => onDelete(confirmation)}>Delete workspace data</button></div></> : <div className={styles.emptyList}><strong>No active workspace profile</strong><span>Import and bind a report/repository before workspace deletion is available.</span></div>}</section></div>;
}

function BindRepositoryDialog({ report, path, scanPrefix, repositoryPrefix, onPathChange, onScanPrefixChange, onRepositoryPrefixChange, onBind, onCancel }: { report: ReportSummary; path: string; scanPrefix: string; repositoryPrefix: string; onPathChange: (value: string) => void; onScanPrefixChange: (value: string) => void; onRepositoryPrefixChange: (value: string) => void; onBind: () => void; onCancel: () => void }) {
  return <div className={styles.modalBackdrop}><section className={styles.modal} role="dialog" aria-modal="true" aria-labelledby="bind-title"><div className={styles.modalHeader}><div><span className={styles.paneKicker}>READ PERMISSION</span><h2 id="bind-title">Bind the scanned repository</h2></div><button className={styles.iconButton} onClick={onCancel} aria-label="Close">×</button></div><p>Choose the local folder that should be inspected for <strong>{report.sourceName}</strong>. This grants read access for the active workspace; it does not authorize patches.</p><label>Repository folder<input autoFocus value={path} onChange={(event) => onPathChange(event.target.value)} placeholder="Select a folder…" /></label><div className={styles.mappingGrid}><label>Scan prefix <input value={scanPrefix} onChange={(event) => onScanPrefixChange(event.target.value)} placeholder="e.g. C:\\agent\\repo" /></label><label>Repository prefix <input value={repositoryPrefix} onChange={(event) => onRepositoryPrefixChange(event.target.value)} placeholder="e.g. packages/web" /></label></div><p className={styles.modalHint}>A prefix mapping is saved as evidence and never selects a file by basename. Ambiguous or unavailable paths remain non-editable.</p><div className={styles.modalActions}><button className={styles.secondaryButton} onClick={onCancel}>Cancel</button><button className={styles.primaryButton} onClick={onBind} disabled={!path}>Confirm repository</button></div></section></div>;
}

function ProposalDialog({ task, investigation, onClose, onCreated }: { task: RemediationTask; investigation: InvestigationBundle; onClose: () => void; onCreated: (review: PatchReview) => void }) {
  const defaultPath = investigation.context.relativePath ?? investigation.finding.filePath ?? task.snapshot.files[0]?.path ?? "";
  const defaultOld = task.snapshot.files.find((file) => file.path === defaultPath)?.content?.slice(0, 0) ?? "";
  const [diagnosis, setDiagnosis] = useState(`Manual proposal for ${investigation.finding.title}`);
  const [path, setPath] = useState(defaultPath);
  const [oldText, setOldText] = useState(defaultOld);
  const [newText, setNewText] = useState("");
  const [behavior, setBehavior] = useState(investigation.playbook.behaviorPreservation[0] ?? "");
  const [tests, setTests] = useState(investigation.playbook.regressionTests[0] ?? "");
  const [expectedAbsent, setExpectedAbsent] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const submit = async () => {
    setSubmitting(true);
    try { const review = await api.createManualProposal(task.id, { diagnosis, assumptions: [], edits: [{ path, oldText, newText, expectedAbsent }], behaviorPreservation: behavior ? [behavior] : [], suggestedTests: tests ? [tests] : [], unresolvedQuestions: [], evidenceRefs: [investigation.finding.rawLocator] }); onCreated(review); } catch (error) { window.alert(errorMessage(error)); } finally { setSubmitting(false); }
  };
  return <div className={styles.modalBackdrop}><section className={`${styles.modal} ${styles.proposalModal}`} role="dialog" aria-modal="true" aria-labelledby="proposal-title"><div className={styles.modalHeader}><div><span className={styles.paneKicker}>USER SUPPLIED · PROPOSAL BUFFER</span><h2 id="proposal-title">Prepare a candidate repair</h2></div><button className={styles.iconButton} onClick={onClose} aria-label="Close">×</button></div><p>These exact text anchors are validated against the captured snapshot. The live file remains read-only until the resulting diff is reviewed and approved.</p><label>Diagnosis<textarea value={diagnosis} onChange={(event) => setDiagnosis(event.target.value)} /></label><div className={styles.mappingGrid}><label>Relative target path<input value={path} onChange={(event) => setPath(event.target.value)} placeholder="src/file.tsx" /></label><label className={styles.checkboxLabel}><input type="checkbox" checked={expectedAbsent} onChange={(event) => setExpectedAbsent(event.target.checked)} /> Create a new file (explicit absence required)</label></div><label>Exact old text anchor {expectedAbsent && <span className={styles.labelMuted}>must be empty</span>}<textarea className={styles.codeInput} value={oldText} onChange={(event) => setOldText(event.target.value)} placeholder={expectedAbsent ? "Leave empty for a new file" : "Paste one unique exact span from the current snapshot"} spellCheck={false} /></label><label>Proposed new text<textarea className={styles.codeInput} value={newText} onChange={(event) => setNewText(event.target.value)} placeholder="The reviewed replacement text" spellCheck={false} /></label><div className={styles.mappingGrid}><label>Behavior rationale<textarea value={behavior} onChange={(event) => setBehavior(event.target.value)} /></label><label>Suggested regression test<textarea value={tests} onChange={(event) => setTests(event.target.value)} /></label></div><div className={styles.modalActions}><button className={styles.secondaryButton} onClick={onClose}>Cancel</button><button className={styles.primaryButton} onClick={() => void submit()} disabled={submitting || !path || (!expectedAbsent && !oldText) || newText === ""}>{submitting ? "Checking anchors…" : "Create reviewed diff"}</button></div></section></div>;
}

function ReviewDialog({ review, reviewed, onReviewed, onApply, onClose, repository }: { review: PatchReview; reviewed: boolean; onReviewed: (value: boolean) => void; onApply: () => void; onClose: () => void; repository?: RepositoryContext }) {
  return <div className={styles.modalBackdrop}><section className={`${styles.modal} ${styles.reviewModal}`} role="dialog" aria-modal="true" aria-labelledby="review-title"><div className={styles.modalHeader}><div><span className={styles.paneKicker}>PATCH REVIEW · {review.touchedFiles.length} FILE{review.touchedFiles.length === 1 ? "" : "S"}</span><h2 id="review-title">Review the actual change</h2></div><button className={styles.iconButton} onClick={onClose} aria-label="Close">×</button></div><div className={styles.reviewBanner}><span className={styles.reviewIcon}>∆</span><div><strong>One approval authorizes this displayed patch transaction only.</strong><span>{repository?.isGitRepository ? "Git preflight will recheck every base hash and apply without staging." : "Git-backed application is unavailable for this folder; export the patch for external review."}</span></div></div><div className={styles.diffFiles}>{review.files.map((file) => <div key={file.path} className={styles.diffFile}><div className={styles.diffFileHeader}><code>{file.path}</code><span>before / after</span></div><DiffSurface file={file} /></div>)}</div><details className={styles.unifiedDetails}><summary>Show canonical unified diff</summary><pre>{review.diff}</pre></details>{review.behaviorRationale.length > 0 && <div className={styles.reviewRationale}><span className={styles.labelObserved}>BEHAVIOR RATIONALE</span>{review.behaviorRationale.map((risk) => <p key={risk}>{risk}</p>)}</div>}{review.risks.length > 0 && <div className={styles.reviewRisks}><span className={styles.labelWarning}>UNRESOLVED RISKS</span>{review.risks.map((risk) => <p key={risk}>! {risk}</p>)}</div>}<label className={styles.reviewCheck}><input type="checkbox" checked={reviewed} onChange={(event) => onReviewed(event.target.checked)} /> I reviewed the complete diff, touched files, behavior rationale, and unresolved risks.</label><div className={styles.modalActions}><button className={styles.secondaryButton} onClick={onClose}>Keep proposal</button><button className={styles.primaryButton} onClick={onApply} disabled={!reviewed || !repository?.isGitRepository}>Apply reviewed patch</button></div>{!repository?.isGitRepository && <p className={styles.modalHint}>This repository is not a Git worktree. CXView keeps the proposal exportable and will not pretend it can safely apply a checked patch here.</p>}</section></div>;
}

function CommandDialog({ candidate, onCancel, onApprove }: { candidate: ValidationCandidate; onCancel: () => void; onApprove: () => void }) {
  return <div className={styles.modalBackdrop}><section className={styles.modal} role="dialog" aria-modal="true" aria-labelledby="command-title"><div className={styles.modalHeader}><div><span className={styles.paneKicker}>COMMAND APPROVAL</span><h2 id="command-title">Run a repository check?</h2></div><button className={styles.iconButton} onClick={onCancel} aria-label="Close">×</button></div><div className={styles.commandPreview}><code>{candidate.executable} {candidate.args.join(" ")}</code><span>cwd: {candidate.workingDirectory}</span><span>{candidate.networkNote}</span>{candidate.expectedWrites.map((write) => <span key={write}>writes: {write}</span>)}</div><p>Repository scripts can invoke lifecycle hooks and arbitrary code. CXView removes known provider API-key variables but cannot sandbox this command.</p><div className={styles.modalActions}><button className={styles.secondaryButton} onClick={onCancel}>Cancel</button><button className={styles.primaryButton} onClick={onApprove}>Approve one run</button></div></section></div>;
}

function ProviderDialog({ provider, hasTask, onClose, onGenerate }: { provider: ProviderDiagnostic; hasTask: boolean; onClose: () => void; onGenerate: () => void }) {
  return <div className={styles.modalBackdrop}><section className={styles.modal} role="dialog" aria-modal="true" aria-labelledby="provider-title"><div className={styles.modalHeader}><div><span className={styles.paneKicker}>PROVIDER DISCLOSURE</span><h2 id="provider-title">Optional proposal adapter</h2></div><button className={styles.iconButton} onClick={onClose} aria-label="Close">×</button></div><p>{provider.provider} is proposal-only. It cannot apply patches, run validation, or change scanner status.</p><div className={styles.storageSummary}><strong>{provider.installed ? "Detected" : "Not installed"}{provider.version ? ` · ${provider.version}` : ""}</strong><span>Executable: {provider.executable ?? "not found on PATH"}</span><span>Structured output: {provider.schemaOutputSupported ? "advertised" : "not verified"} · read-only flag: {provider.readOnlyFlagSupported ? "advertised" : "not verified"}</span></div><div className={styles.disclosure}><span className={styles.labelScanner}>AUTHENTICATION</span><p>{provider.authentication}</p><span className={styles.labelScanner}>PERMISSIONS</span><p>{provider.permissions}</p><span className={styles.labelScanner}>DATA DESTINATION</span><p>{provider.dataDestination}</p></div><p className={styles.modalHint}>{provider.diagnostic}</p>{provider.integrationEnabled ? <button className={styles.primaryButton} onClick={onGenerate} disabled={!hasTask || !api.isNative}>Generate schema-validated proposal</button> : <p className={styles.modalHint}>Integrated generation is disabled until CXView can enforce the provider boundary. Use “Export task” and “Import proposal” for an explicit external handoff.</p>}<div className={styles.modalActions}><button className={styles.secondaryButton} onClick={onClose}>Close</button></div></section></div>;
}

function ValidationDrawer({ candidates, runs, onClose, onRun }: { candidates: ValidationCandidate[]; runs: ValidationRun[]; onClose: () => void; onRun: (candidate: ValidationCandidate) => void }) {
  return <section className={styles.validationDrawer} aria-label="Local validation drawer"><div className={styles.validationHeader}><div><span className={styles.paneKicker}>LOCAL VALIDATION EVIDENCE</span><h2>Checks from the actual manifest</h2></div><button className={styles.iconButton} onClick={onClose} aria-label="Close validation drawer">×</button></div><div className={styles.validationGrid}><div><p className={styles.modalHint}>No command was invented. Each candidate below came from package.json and may run lifecycle hooks or use the network.</p>{candidates.map((candidate) => <div className={styles.validationCandidate} key={candidate.id}><div><strong>{candidate.id.replace("script-", "")}</strong><code>{candidate.executable} {candidate.args.join(" ")}</code></div><button className={styles.secondaryButton} onClick={() => onRun(candidate)}>Approve + run</button></div>)}{candidates.length === 0 && <div className={styles.emptyList}><strong>No check candidates</strong><span>There is no supported script in this package root.</span></div>}</div><div><div className={styles.sectionTitle}>Recorded runs <span>bound to task snapshot</span></div>{runs.map((run) => <div className={styles.runRow} key={run.id}><span className={`${styles.runStatus} ${run.status === "passed" ? styles.runPassed : run.status === "failed" ? styles.runFailed : styles.runUnknown}`}>{run.status}</span><div><strong>{run.candidateId}</strong><span>{run.durationMs} ms · exit {run.exitCode ?? "—"}</span><p>{run.note}</p>{run.stderr && <pre>{run.stderr}</pre>}{run.stdout && <pre>{run.stdout}</pre>}</div></div>)}{runs.length === 0 && <div className={styles.emptyList}><strong>No validation evidence yet</strong><span>Run a baseline or post-change check after reviewing its command.</span></div>}</div></div></section>;
}

function UnselectedState({ hasRepository, onBind }: { hasRepository: boolean; onBind: () => void }) { return <div className={styles.unselected}><div className={styles.unselectedIcon}>⌁</div><h2>Choose a finding</h2><p>{hasRepository ? "The center pane will show scanner nodes beside current read-only source." : "Bind a repository to unlock current-code evidence. Report evidence remains visible without one."}</p>{!hasRepository && <button className={styles.secondaryButton} onClick={onBind}>Bind repository</button>}</div>; }

function errorMessage(error: unknown): string { return error instanceof Error ? error.message : typeof error === "string" ? error : "CXView could not complete that operation."; }
function emptyProvider(): ProviderDiagnostic { return { provider: "Codex CLI", installed: false, schemaOutputSupported: false, readOnlyFlagSupported: false, integrationEnabled: false, authentication: "Not checked", permissions: "Not checked", dataDestination: "No automatic outbound requests", diagnostic: "Provider diagnostic unavailable in browser preview." }; }
function preventDefault(event: Event) { event.preventDefault(); }
function shortPath(path: string) { const parts = path.replaceAll("\\", "/").split("/"); return parts.length > 3 ? `…/${parts.slice(-2).join("/")}` : path; }
function capitalize(value: string) { return value.slice(0, 1).toUpperCase() + value.slice(1); }
function severityClass(severity: string) { return `${styles[`severity${severity}`] ?? styles.severityUnknown}`; }
function stateClass(state: string) { return styles[`match${state}`] ?? styles.matchUnavailable; }
function taskStateClass(state: TaskState) { return styles[`task${state}`] ?? styles.taskInvestigating; }
function readinessLabel(readiness: FindingSummary["evidenceReadiness"]) { return readiness === "ready" ? "evidence ready" : readiness === "partial" ? "partial evidence" : readiness === "ambiguous" ? "ambiguous" : "evidence missing"; }
function viewLabel(value: string) { return value.replaceAll("-", " ").replace(/\b\w/g, (letter) => letter.toUpperCase()); }
function clearFilter(filter: string, setSeverity: (value: string) => void, setEngine: (value: string) => void, setStatus: (value: string) => void, setView: (value: string) => void) { if (filter.startsWith("severity:")) setSeverity(""); else if (filter.startsWith("engine:")) setEngine(""); else if (filter.startsWith("status:")) setStatus(""); else setView(""); }
function formatBytes(bytes: number) { if (bytes < 1024) return `${bytes} B`; if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`; return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`; }
function clamp(value: number, minimum: number, maximum: number) { return Math.max(minimum, Math.min(maximum, value)); }

export default App;
