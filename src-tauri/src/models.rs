use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSummary {
    pub id: String,
    pub source_name: String,
    pub source_path: String,
    pub sha256: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub imported_at: String,
    pub finding_count: usize,
    pub scanner_sections: Vec<String>,
    pub metadata: ReportMetadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportMetadata {
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub scan_id: Option<String>,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub timestamp: Option<String>,
    pub included_engines: Vec<String>,
    pub filters: Option<Value>,
    pub scan_status: Option<String>,
    pub completeness: Option<String>,
    pub scope: Option<Value>,
    pub declared_counts: Vec<DeclaredCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredCount {
    pub label: String,
    pub value: usize,
    pub locator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingSummary {
    pub id: String,
    pub report_id: String,
    pub fingerprint: String,
    pub engine: String,
    pub scanner: Option<String>,
    pub rule: Option<String>,
    pub title: String,
    pub severity: String,
    pub original_severity: Option<String>,
    pub result_status: Option<String>,
    pub triage_state: Option<String>,
    pub category: FindingCategory,
    pub file_path: Option<String>,
    pub package_name: Option<String>,
    pub package_version: Option<String>,
    pub scan_package_version: Option<String>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    pub evidence_readiness: EvidenceReadiness,
    pub local_task_state: TaskState,
    pub raw_locator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FindingCategory {
    Sast,
    Sca,
    Iac,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EvidenceReadiness {
    Ready,
    Partial,
    Missing,
    Ambiguous,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeEvidence {
    pub order: usize,
    pub path: Option<String>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    pub column_start: Option<u32>,
    pub column_end: Option<u32>,
    pub role: Option<String>,
    pub source_sink: Option<String>,
    pub snippet: Option<String>,
    pub locator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingRecord {
    #[serde(flatten)]
    pub summary: FindingSummary,
    pub description: Option<String>,
    pub recommendation: Option<String>,
    pub cwe: Option<String>,
    pub query_id: Option<String>,
    pub query_name: Option<String>,
    pub evidence_links: Vec<String>,
    pub nodes: Vec<NodeEvidence>,
    pub reported_snippet: Option<String>,
    pub advisory_aliases: Vec<String>,
    pub ecosystem: Option<String>,
    pub affected_range: Option<String>,
    pub fixed_range: Option<String>,
    pub dependency_paths: Vec<String>,
    pub reachability: Option<String>,
    pub resource: Option<String>,
    pub iac_rule: Option<String>,
    pub expected_value: Option<String>,
    pub actual_value: Option<String>,
    pub provider: Option<String>,
    pub context: Option<String>,
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsupportedRecord {
    pub locator: String,
    pub reason: String,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MalformedRecord {
    pub locator: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDiagnostics {
    pub format: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub scanner_sections: Vec<String>,
    pub parsed_instances: usize,
    pub parsed_by_engine: ParsedCounts,
    pub unsupported_records: Vec<UnsupportedRecord>,
    pub malformed_records: Vec<MalformedRecord>,
    pub count_mismatches: Vec<CountMismatch>,
    pub evidence_notes: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawInspection {
    pub report_id: String,
    pub locator: String,
    pub source_path: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedWorkspaceData {
    pub raw_paths: Vec<String>,
    pub patch_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedCounts {
    pub sast: usize,
    pub sca: usize,
    pub iac: usize,
    pub unknown: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountMismatch {
    pub label: String,
    pub declared: usize,
    pub parsed: usize,
    pub locator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub report: ReportSummary,
    pub diagnostics: ImportDiagnostics,
    pub findings: Vec<FindingSummary>,
    pub comparison: Option<ComparisonSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub repository_path: Option<String>,
    pub report_id: Option<String>,
    pub scan_prefix: Option<String>,
    pub repository_prefix: Option<String>,
    pub ui_state: UiState,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    pub selected_finding_id: Option<String>,
    pub search: String,
    pub severity_filter: Option<String>,
    pub engine_filter: Option<String>,
    pub status_filter: Option<String>,
    pub saved_view: Option<String>,
    pub left_width: Option<u32>,
    pub right_width: Option<u32>,
    pub theme: Option<String>,
    pub code_font_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryContext {
    pub path: String,
    pub canonical_path: String,
    pub is_git_repository: bool,
    pub git_root: Option<String>,
    pub branch: Option<String>,
    pub head_commit: Option<String>,
    pub staged_files: Vec<String>,
    pub unstaged_files: Vec<String>,
    pub package_root: Option<String>,
    pub package_manager: Option<String>,
    pub package_manager_support: PackageManagerSupport,
    pub prefix_mapping: Option<PrefixMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefixMapping {
    pub scan_prefix: String,
    pub repository_prefix: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageManagerSupport {
    pub name: String,
    pub lockfile: Option<String>,
    pub supported_lockfile: bool,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchState {
    Matched,
    RelocatedWithEvidence,
    Ambiguous,
    Unavailable,
    CurrentFileDiffers,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeContext {
    pub finding_id: String,
    pub match_state: MatchState,
    pub resolved_path: Option<String>,
    pub relative_path: Option<String>,
    pub current_source: Option<String>,
    pub current_range_start: Option<u32>,
    pub current_range_end: Option<u32>,
    pub reported_snippet: Option<String>,
    pub reported_nodes: Vec<NodeEvidence>,
    pub observed_local: Vec<EvidenceStatement>,
    pub unknowns: Vec<String>,
    pub syntax: Option<SyntaxContext>,
    pub related_files: Vec<String>,
    pub source_hash: Option<String>,
    pub sca: Option<DependencyContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvestigationBundle {
    pub finding: FindingRecord,
    pub context: CodeContext,
    pub playbook: RemediationPlaybook,
    pub local_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemediationPlaybook {
    pub id: String,
    pub title: String,
    pub last_reviewed: String,
    pub applicability: Vec<String>,
    pub investigation_questions: Vec<String>,
    pub preferred_repairs: Vec<String>,
    pub behavior_preservation: Vec<String>,
    pub regression_tests: Vec<String>,
    pub contraindications: Vec<String>,
    pub references: Vec<ReferenceLink>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceLink {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceStatement {
    pub label: EvidenceLabel,
    pub text: String,
    pub locator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceLabel {
    Scanner,
    ObservedLocally,
    UserSupplied,
    AiHypothesis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntaxContext {
    pub parser: String,
    pub parser_version: String,
    pub syntax_error_count: usize,
    pub imports: Vec<String>,
    pub enclosing_declaration: Option<String>,
    pub parser_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyContext {
    pub package_name: Option<String>,
    pub scan_version: Option<String>,
    pub resolved_instances: Vec<ResolvedDependency>,
    pub direct_dependency: Option<bool>,
    pub ownership_paths: Vec<String>,
    pub package_json_evidence: Vec<String>,
    pub inspection_commands: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDependency {
    pub install_path: String,
    pub version: Option<String>,
    pub dependency_context: Option<String>,
    pub dependency_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Investigating,
    ProposalReady,
    Applied,
    LocallyValidated,
    AwaitingRescan,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemediationTask {
    pub id: String,
    pub profile_id: String,
    pub report_id: String,
    pub finding_ids: Vec<String>,
    pub state: TaskState,
    pub created_at: String,
    pub updated_at: String,
    pub snapshot: SnapshotManifest,
    pub proposal: Option<ProposalDocument>,
    pub diff: Option<String>,
    pub patch_id: Option<String>,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotManifest {
    pub task_id: String,
    pub report_id: String,
    pub repository_path: String,
    pub branch: Option<String>,
    pub head_commit: Option<String>,
    pub captured_at: String,
    pub files: Vec<FileManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileManifest {
    pub path: String,
    pub sha256: String,
    pub byte_length: usize,
    pub line_ending: String,
    pub content: Option<String>,
    pub expected_absent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalDocument {
    pub schema_version: String,
    pub task_id: String,
    pub snapshot_id: String,
    pub source: ProposalSource,
    pub provider: Option<String>,
    pub diagnosis: String,
    pub assumptions: Vec<String>,
    pub edits: Vec<TextEdit>,
    pub behavior_preservation: Vec<String>,
    pub suggested_tests: Vec<String>,
    pub unresolved_questions: Vec<String>,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProposalSource {
    Manual,
    Imported,
    OfflinePlaybook,
    Codex,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    pub path: String,
    pub old_text: String,
    pub new_text: String,
    pub expected_absent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchReview {
    pub task_id: String,
    pub patch_id: String,
    pub diff: String,
    pub touched_files: Vec<String>,
    pub review_ready: bool,
    pub risks: Vec<String>,
    pub behavior_rationale: Vec<String>,
    pub files: Vec<DiffFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    pub path: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedPatch {
    pub patch_id: String,
    pub task_id: String,
    pub applied_at: String,
    pub touched_files: Vec<String>,
    pub post_hashes: Vec<FileHash>,
    pub undo_available: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHash {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationCandidate {
    pub id: String,
    pub label: String,
    pub executable: String,
    pub args: Vec<String>,
    pub working_directory: String,
    pub source: String,
    pub script_definition_hash: Option<String>,
    pub expected_writes: Vec<String>,
    pub network_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationRun {
    pub id: String,
    pub task_id: String,
    pub candidate_id: String,
    pub status: ValidationStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    pub stdout: String,
    pub stderr: String,
    pub snapshot_hash: String,
    pub started_at: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValidationStatus {
    Passed,
    Failed,
    Missing,
    Canceled,
    TimedOut,
    Skipped,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDiagnostic {
    pub provider: String,
    pub executable: Option<String>,
    pub version: Option<String>,
    pub installed: bool,
    pub schema_output_supported: bool,
    pub read_only_flag_supported: bool,
    pub integration_enabled: bool,
    pub authentication: String,
    pub permissions: String,
    pub data_destination: String,
    pub diagnostic: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonSummary {
    pub baseline_report_id: String,
    pub compared_report_id: String,
    pub provenance_comparable: bool,
    pub provenance_reason: String,
    pub newly_observed: usize,
    pub still_observed: usize,
    pub absent_under_comparable_scope: usize,
    pub explicitly_reported_fixed: usize,
    pub incomparable: usize,
    pub items: Vec<ComparisonItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonItem {
    pub finding_id: String,
    pub classification: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub profile: Option<Profile>,
    pub report: Option<ReportSummary>,
    pub diagnostics: Option<ImportDiagnostics>,
    pub findings: Vec<FindingSummary>,
    pub repository: Option<RepositoryContext>,
    pub tasks: Vec<RemediationTask>,
    pub provider: ProviderDiagnostic,
}
