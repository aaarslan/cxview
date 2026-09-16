export type FindingCategory = "sast" | "sca" | "iac" | "unknown";
export type EvidenceReadiness = "ready" | "partial" | "missing" | "ambiguous";
export type TaskState = "investigating" | "proposalready" | "applied" | "locallyvalidated" | "awaitingrescan" | "blocked" | "failed";
export type MatchState = "matched" | "relocatedwithevidence" | "ambiguous" | "unavailable" | "currentfilediffers" | "notapplicable";
export type ValidationStatus = "passed" | "failed" | "missing" | "canceled" | "timedout" | "skipped" | "unknown";

export interface ReportMetadata {
  projectId?: string;
  projectName?: string;
  scanId?: string;
  branch?: string;
  commit?: string;
  timestamp?: string;
  includedEngines: string[];
  filters?: unknown;
  scanStatus?: string;
  completeness?: string;
  scope?: unknown;
  declaredCounts: DeclaredCount[];
}

export interface DeclaredCount { label: string; value: number; locator: string }

export interface ReportSummary {
  id: string;
  sourceName: string;
  sourcePath: string;
  sha256: string;
  adapterId: string;
  adapterVersion: string;
  importedAt: string;
  findingCount: number;
  scannerSections: string[];
  metadata: ReportMetadata;
}

export interface FindingSummary {
  id: string;
  reportId: string;
  fingerprint: string;
  engine: string;
  scanner?: string;
  rule?: string;
  title: string;
  severity: string;
  originalSeverity?: string;
  resultStatus?: string;
  triageState?: string;
  category: FindingCategory;
  filePath?: string;
  packageName?: string;
  packageVersion?: string;
  scanPackageVersion?: string;
  lineStart?: number;
  lineEnd?: number;
  evidenceReadiness: EvidenceReadiness;
  localTaskState: TaskState;
  rawLocator: string;
}

export interface NodeEvidence {
  order: number;
  path?: string;
  lineStart?: number;
  lineEnd?: number;
  columnStart?: number;
  columnEnd?: number;
  role?: string;
  sourceSink?: string;
  snippet?: string;
  locator: string;
}

export interface FindingRecord extends FindingSummary {
  description?: string;
  recommendation?: string;
  cwe?: string;
  queryId?: string;
  queryName?: string;
  evidenceLinks: string[];
  nodes: NodeEvidence[];
  reportedSnippet?: string;
  advisoryAliases: string[];
  ecosystem?: string;
  affectedRange?: string;
  fixedRange?: string;
  dependencyPaths: string[];
  reachability?: string;
  resource?: string;
  iacRule?: string;
  expectedValue?: string;
  actualValue?: string;
  provider?: string;
  context?: string;
  raw: unknown;
}

export interface ImportDiagnostics {
  format: string;
  adapterId: string;
  adapterVersion: string;
  scannerSections: string[];
  parsedInstances: number;
  parsedByEngine: { sast: number; sca: number; iac: number; unknown: number };
  unsupportedRecords: { locator: string; reason: string; preview: string }[];
  malformedRecords: { locator: string; reason: string }[];
  countMismatches: { label: string; declared: number; parsed: number; locator: string }[];
  evidenceNotes: string[];
  warnings: string[];
}
export interface RawInspection { reportId: string; locator: string; sourcePath: string; value: unknown }
export interface StorageInfo { root: string; bytes: number; retainedObjects: string[]; note: string }

export interface PrefixMapping { scanPrefix: string; repositoryPrefix: string; reason: string }
export interface PackageManagerSupport { name: string; lockfile?: string; supportedLockfile: boolean; note: string }
export interface RepositoryContext {
  path: string;
  canonicalPath: string;
  isGitRepository: boolean;
  gitRoot?: string;
  branch?: string;
  headCommit?: string;
  stagedFiles: string[];
  unstagedFiles: string[];
  packageRoot?: string;
  packageManager?: string;
  packageManagerSupport: PackageManagerSupport;
  prefixMapping?: PrefixMapping;
}

export interface EvidenceStatement { label: "scanner" | "observedLocally" | "userSupplied" | "aiHypothesis"; text: string; locator?: string }
export interface SyntaxContext { parser: string; parserVersion: string; syntaxErrorCount: number; imports: string[]; enclosingDeclaration?: string; parserNote: string }
export interface ResolvedDependency { installPath: string; version?: string; dependencyContext?: string; dependencyType?: string }
export interface DependencyContext {
  packageName?: string;
  scanVersion?: string;
  resolvedInstances: ResolvedDependency[];
  directDependency?: boolean;
  ownershipPaths: string[];
  packageJsonEvidence: string[];
  inspectionCommands: string[];
  notes: string[];
}
export interface CodeContext {
  findingId: string;
  matchState: MatchState;
  resolvedPath?: string;
  relativePath?: string;
  currentSource?: string;
  currentRangeStart?: number;
  currentRangeEnd?: number;
  reportedSnippet?: string;
  reportedNodes: NodeEvidence[];
  observedLocal: EvidenceStatement[];
  unknowns: string[];
  syntax?: SyntaxContext;
  relatedFiles: string[];
  sourceHash?: string;
  sca?: DependencyContext;
}
export interface ReferenceLink { label: string; url: string }
export interface RemediationPlaybook { id: string; title: string; lastReviewed: string; applicability: string[]; investigationQuestions: string[]; preferredRepairs: string[]; behaviorPreservation: string[]; regressionTests: string[]; contraindications: string[]; references: ReferenceLink[]; note: string }
export interface InvestigationBundle { finding: FindingRecord; context: CodeContext; playbook: RemediationPlaybook; localNote?: string }

export interface UiState { selectedFindingId?: string; search: string; severityFilter?: string; engineFilter?: string; statusFilter?: string; savedView?: string; leftWidth?: number; rightWidth?: number; theme?: string; codeFontSize?: number }
export interface Profile { id: string; name: string; repositoryPath?: string; reportId?: string; scanPrefix?: string; repositoryPrefix?: string; uiState: UiState }
export interface FileManifest { path: string; sha256: string; byteLength: number; lineEnding: string; content?: string; expectedAbsent: boolean }
export interface SnapshotManifest { taskId: string; reportId: string; repositoryPath: string; branch?: string; headCommit?: string; capturedAt: string; files: FileManifest[] }
export interface TextEdit { path: string; oldText: string; newText: string; expectedAbsent: boolean }
export interface ProposalDocument { schemaVersion: string; taskId: string; snapshotId: string; source: "manual" | "imported" | "offlineplaybook" | "codex"; provider?: string; diagnosis: string; assumptions: string[]; edits: TextEdit[]; behaviorPreservation: string[]; suggestedTests: string[]; unresolvedQuestions: string[]; evidenceRefs: string[] }
export interface RemediationTask { id: string; profileId: string; reportId: string; findingIds: string[]; state: TaskState; createdAt: string; updatedAt: string; snapshot: SnapshotManifest; proposal?: ProposalDocument; diff?: string; patchId?: string; notes: string }
export interface DiffFile { path: string; before: string; after: string }
export interface PatchReview { taskId: string; patchId: string; diff: string; touchedFiles: string[]; reviewReady: boolean; risks: string[]; behaviorRationale: string[]; files: DiffFile[] }
export interface AppliedPatch { patchId: string; taskId: string; appliedAt: string; touchedFiles: string[]; postHashes: { path: string; sha256: string }[]; undoAvailable: boolean; message: string }
export interface ValidationCandidate { id: string; label: string; executable: string; args: string[]; workingDirectory: string; source: string; scriptDefinitionHash?: string; expectedWrites: string[]; networkNote: string }
export interface ValidationRun { id: string; taskId: string; candidateId: string; status: ValidationStatus; exitCode?: number; durationMs: number; stdout: string; stderr: string; snapshotHash: string; startedAt: string; note: string }
export interface ProviderDiagnostic { provider: string; executable?: string; version?: string; installed: boolean; schemaOutputSupported: boolean; readOnlyFlagSupported: boolean; integrationEnabled: boolean; authentication: string; permissions: string; dataDestination: string; diagnostic: string }
export interface ComparisonSummary { baselineReportId: string; comparedReportId: string; provenanceComparable: boolean; provenanceReason: string; newlyObserved: number; stillObserved: number; absentUnderComparableScope: number; explicitlyReportedFixed: number; incomparable: number; items: { findingId: string; classification: string; reason: string }[] }
export interface ImportResult { report: ReportSummary; diagnostics: ImportDiagnostics; findings: FindingSummary[]; comparison?: ComparisonSummary }
export interface AppSnapshot { profile?: Profile; report?: ReportSummary; diagnostics?: ImportDiagnostics; findings: FindingSummary[]; repository?: RepositoryContext; tasks: RemediationTask[]; provider: ProviderDiagnostic }
