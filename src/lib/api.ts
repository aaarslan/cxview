import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { AppSnapshot, AppliedPatch, ComparisonSummary, FindingSummary, ImportResult, InvestigationBundle, PatchReview, Profile, ProviderDiagnostic, RawInspection, RemediationTask, RepositoryContext, ReportSummary, StorageInfo, UiState, ValidationCandidate, ValidationRun } from "./types";

export const isNative = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isNative) throw new Error("CXView native commands are unavailable in a browser preview. Run `pnpm tauri dev` for the desktop workflow.");
  return tauriInvoke<T>(command, args);
}

export async function chooseReport(): Promise<string | null> {
  if (!isNative) return null;
  const selected = await open({ multiple: false, directory: false, filters: [{ name: "JSON report", extensions: ["json"] }] });
  return typeof selected === "string" ? selected : null;
}

export async function chooseRepository(): Promise<string | null> {
  if (!isNative) return null;
  const selected = await open({ multiple: false, directory: true });
  return typeof selected === "string" ? selected : null;
}

export async function chooseProposal(): Promise<string | null> {
  if (!isNative) return null;
  const selected = await open({ multiple: false, directory: false, filters: [{ name: "CXView proposal", extensions: ["json"] }] });
  return typeof selected === "string" ? selected : null;
}

export async function chooseExportPath(defaultPath: string): Promise<string | null> {
  if (!isNative) return null;
  const selected = await save({ defaultPath, filters: [{ name: "JSON", extensions: ["json"] }] });
  return selected ?? null;
}

export async function loadSnapshot(): Promise<AppSnapshot> { return invoke<AppSnapshot>("get_app_snapshot"); }
export async function importReport(path: string): Promise<ImportResult> { return invoke<ImportResult>("import_report", { path }); }
export async function bindRepository(request: { reportId: string; repositoryPath: string; scanPrefix?: string; repositoryPrefix?: string }): Promise<RepositoryContext> { return invoke<RepositoryContext>("bind_repository", { request }); }
export async function listFindings(request: { reportId: string; search?: string; severity?: string; engine?: string; status?: string; savedView?: string; page?: number; pageSize?: number }): Promise<FindingSummary[]> { return invoke<FindingSummary[]>("list_findings", { query: request }); }
export async function readRawLocator(reportId: string, locator: string): Promise<RawInspection> { return invoke<RawInspection>("read_raw_locator", { reportId, locator }); }
export async function investigation(findingId: string): Promise<InvestigationBundle> { return invoke<InvestigationBundle>("get_investigation", { findingId }); }
export async function createTask(findingId: string): Promise<RemediationTask> { return invoke<RemediationTask>("create_task", { findingId }); }
export async function getTask(taskId: string): Promise<RemediationTask> { return invoke<RemediationTask>("get_task", { taskId }); }
export async function createManualProposal(taskId: string, input: unknown): Promise<PatchReview> { return invoke<PatchReview>("create_manual_proposal", { taskId, input }); }
export async function importProposal(taskId: string, path: string): Promise<PatchReview> { return invoke<PatchReview>("import_proposal", { taskId, path }); }
export async function reviewPatch(taskId: string): Promise<PatchReview> { return invoke<PatchReview>("review_patch", { taskId }); }
export async function applyReviewedPatch(request: { taskId: string; patchId: string; reviewed: boolean }): Promise<AppliedPatch> { return invoke<AppliedPatch>("apply_reviewed_patch", { request }); }
export async function undoPatch(taskId: string): Promise<string[]> { return invoke<string[]>("undo_cxview_patch", { taskId }); }
export async function exportTask(taskId: string, destination: string): Promise<string> { return invoke<string>("export_task", { taskId, destination }); }
export async function investigationPrompt(findingId: string): Promise<string> { return invoke<string>("investigation_prompt", { findingId }); }
export async function discoverValidation(taskId: string): Promise<ValidationCandidate[]> { return invoke<ValidationCandidate[]>("discover_validation", { taskId }); }
export async function runValidation(request: { taskId: string; candidateId: string; approved: boolean }): Promise<ValidationRun> { return invoke<ValidationRun>("run_validation", { request }); }
export async function validationHistory(taskId: string): Promise<ValidationRun[]> { return invoke<ValidationRun[]>("validation_history", { taskId }); }
export async function saveNote(findingId: string, note: string): Promise<void> { return invoke<void>("save_finding_note", { findingId, note }); }
export async function saveUiState(profileId: string, state: UiState): Promise<void> { return invoke<void>("save_ui_state", { request: { profileId, state } }); }
export async function providerDiagnostic(): Promise<ProviderDiagnostic> { return invoke<ProviderDiagnostic>("provider_diagnostic"); }
export async function runCodexProposal(taskId: string): Promise<PatchReview> { return invoke<PatchReview>("run_codex_proposal", { taskId }); }
export async function compareReports(baselineReportId: string, comparedReportId: string): Promise<ComparisonSummary> { return invoke<ComparisonSummary>("compare_report_pair", { baselineReportId, comparedReportId }); }
export async function storageInfo(): Promise<StorageInfo> { return invoke<StorageInfo>("storage_info"); }
export async function deleteWorkspaceData(profileId: string, confirmation: string): Promise<StorageInfo> { return invoke<StorageInfo>("delete_workspace_data", { request: { profileId, confirmation } }); }

export function reportLabel(report?: ReportSummary): string { return report ? `${report.sourceName} · ${report.findingCount} parsed instances` : "No report imported"; }
export function profileLabel(profile?: Profile): string { return profile?.repositoryPath ?? "No repository bound"; }
