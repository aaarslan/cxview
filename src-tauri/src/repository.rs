use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use serde_json::Value;
use walkdir::WalkDir;

use crate::error::{AppError, AppResult};
use crate::importer::sha256_hex;
use crate::models::*;

const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;

pub fn inspect_repository(
    path: &str,
    scan_prefix: Option<&str>,
    repository_prefix: Option<&str>,
) -> AppResult<RepositoryContext> {
    let requested = PathBuf::from(path);
    if !requested.exists() {
        return Err(AppError::MissingPath(path.to_owned()));
    }
    let canonical = requested.canonicalize().map_err(|source| AppError::Io {
        path: requested.clone(),
        source,
    })?;
    if !canonical.is_dir() {
        return Err(AppError::Message(
            "repository selection must be a folder".to_owned(),
        ));
    }
    let git_root = git_value(&canonical, &["rev-parse", "--show-toplevel"]).map(|value| {
        PathBuf::from(value.clone())
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(value))
    });
    let is_git_repository = git_root.is_some();
    let branch = git_value(&canonical, &["branch", "--show-current"]);
    let head_commit = git_value(&canonical, &["rev-parse", "HEAD"]);
    let staged_files = git_lines(&canonical, &["diff", "--cached", "--name-only"]);
    let unstaged_files = git_lines(&canonical, &["diff", "--name-only"]);
    let package_root = nearest_package_root(&canonical, None);
    let (package_manager, package_manager_support) =
        detect_package_manager(package_root.as_deref());
    let prefix_mapping = match (scan_prefix.map(str::trim).filter(|s| !s.is_empty()), repository_prefix.map(str::trim).filter(|s| !s.is_empty())) {
        (Some(scan), Some(repo)) => Some(PrefixMapping { scan_prefix: scan.to_owned(), repository_prefix: repo.to_owned(), reason: "User supplied mapping; it is saved for this profile and is not an edit authorization.".to_owned() }),
        _ => None,
    };
    Ok(RepositoryContext {
        path: path.to_owned(),
        canonical_path: canonical.to_string_lossy().into_owned(),
        is_git_repository,
        git_root: git_root.map(|path| path.to_string_lossy().into_owned()),
        branch,
        head_commit,
        staged_files,
        unstaged_files,
        package_root: package_root.map(|path| path.to_string_lossy().into_owned()),
        package_manager,
        package_manager_support,
        prefix_mapping,
    })
}

pub fn repository_root(path: &str) -> AppResult<PathBuf> {
    let requested = PathBuf::from(path);
    let canonical = requested.canonicalize().map_err(|source| AppError::Io {
        path: requested.clone(),
        source,
    })?;
    if !canonical.is_dir() {
        return Err(AppError::MissingPath(path.to_owned()));
    }
    // The selected folder is the containment/scope root. Git may report a parent
    // monorepo root, but CXView must not silently widen source access to it.
    Ok(canonical)
}

pub fn safe_relative_path(root: &Path, relative: &str, allow_absent: bool) -> AppResult<PathBuf> {
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_owned());
    let normalized = relative.replace('\\', "/");
    let candidate = Path::new(&normalized);
    if candidate.is_absolute()
        || normalized.as_bytes().get(1) == Some(&b':')
        || normalized.starts_with('/')
        || normalized.split('/').any(|component| component == "..")
    {
        return Err(AppError::OutsideRepository(relative.to_owned()));
    }
    let first = normalized.split('/').next().unwrap_or_default();
    if first == ".git" || normalized == ".git" || normalized.starts_with(".git/") {
        return Err(AppError::UnsafePath(
            ".git paths are not editable".to_owned(),
        ));
    }
    let joined = canonical_root.join(candidate);
    if joined.exists() {
        reject_symlink_ancestors(&canonical_root, &joined)?;
        let canonical = joined.canonicalize().map_err(|source| AppError::Io {
            path: joined.clone(),
            source,
        })?;
        if !canonical.starts_with(&canonical_root) {
            return Err(AppError::OutsideRepository(relative.to_owned()));
        }
        if canonical.is_dir() {
            return Err(AppError::Unsupported(
                "directories are not editable proposal targets".to_owned(),
            ));
        }
        Ok(canonical)
    } else if allow_absent {
        let parent = joined
            .parent()
            .ok_or_else(|| AppError::OutsideRepository(relative.to_owned()))?;
        let canonical_parent = parent.canonicalize().map_err(|source| AppError::Io {
            path: parent.to_owned(),
            source,
        })?;
        if !canonical_parent.starts_with(&canonical_root) {
            return Err(AppError::OutsideRepository(relative.to_owned()));
        }
        reject_symlink_ancestors(&canonical_root, parent)?;
        Ok(joined)
    } else {
        Err(AppError::MissingPath(relative.to_owned()))
    }
}

fn reject_symlink_ancestors(root: &Path, path: &Path) -> AppResult<()> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut current = root.to_owned();
    for component in relative.components() {
        current.push(component.as_os_str());
        if fs::symlink_metadata(&current)
            .map(|metadata| is_link_or_reparse(&metadata))
            .unwrap_or(false)
        {
            return Err(AppError::UnsafePath(format!(
                "symbolic link or reparse point in {}",
                current.display()
            )));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

pub fn resolve_finding_path(
    root: &Path,
    report_path: Option<&str>,
    scan_prefix: Option<&str>,
    repository_prefix: Option<&str>,
    reported_snippet: Option<&str>,
) -> AppResult<ResolvedPath> {
    let Some(report_path) = report_path.filter(|path| !path.trim().is_empty()) else {
        return Ok(ResolvedPath {
            state: MatchState::Unavailable,
            absolute: None,
            relative: None,
            reason: "The finding has no file path evidence.".to_owned(),
            snippet_evidence: false,
        });
    };
    let normalized = normalize_report_path(report_path);
    let mut relative = normalized.clone();
    let mut mapped = false;
    if let Some(prefix) = scan_prefix
        .map(normalize_report_path)
        .filter(|prefix| !prefix.is_empty())
    {
        if relative == prefix {
            relative.clear();
            mapped = true;
        } else if let Some(rest) = relative.strip_prefix(&(prefix.clone() + "/")) {
            relative = rest.to_owned();
            mapped = true;
        }
    }
    if let Some(prefix) = repository_prefix
        .map(normalize_report_path)
        .filter(|prefix| !prefix.is_empty())
    {
        relative = format!("{prefix}/{}", relative.trim_start_matches('/'));
        mapped = true;
    }
    while relative.starts_with("./") {
        relative = relative[2..].to_owned();
    }
    if let Some(stripped) = relative.strip_prefix("./") {
        relative = stripped.to_owned();
    }
    let direct = if !relative.is_empty() {
        safe_relative_path(root, &relative, false).ok()
    } else {
        None
    };
    if let Some(path) = direct {
        let current = fs::read(&path).map_err(|source| AppError::Io {
            path: path.clone(),
            source,
        })?;
        let text = String::from_utf8(current).ok();
        let differs = text
            .as_deref()
            .zip(reported_snippet)
            .map(|(source, snippet)| !snippet.trim().is_empty() && !source.contains(snippet.trim()))
            .unwrap_or(false);
        let state = if differs {
            MatchState::CurrentFileDiffers
        } else if mapped {
            MatchState::RelocatedWithEvidence
        } else {
            MatchState::Matched
        };
        return Ok(ResolvedPath {
            state,
            absolute: Some(path),
            relative: Some(relative),
            reason: if differs {
                "The mapped file exists, but the reported snippet is not present in current bytes."
                    .to_owned()
            } else if mapped {
                "The saved prefix mapping resolves the report path to this repository file."
                    .to_owned()
            } else {
                "The report path resolves exactly inside the selected repository.".to_owned()
            },
            snippet_evidence: false,
        });
    }
    let filename = Path::new(&relative)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned();
    if filename.is_empty() {
        return Ok(ResolvedPath {
            state: MatchState::Unavailable,
            absolute: None,
            relative: None,
            reason: "The report path has no usable filename.".to_owned(),
            snippet_evidence: false,
        });
    }
    let snippet = reported_snippet
        .map(str::trim)
        .filter(|snippet| !snippet.is_empty());
    let mut candidates = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .max_depth(12)
        .into_iter()
        .filter_map(Result::ok)
        .take(5000)
    {
        if !entry.file_type().is_file() || entry.file_name().to_string_lossy() != filename {
            continue;
        }
        let path = entry.path().to_owned();
        if let Some(snippet) = snippet {
            if let Ok(bytes) = fs::read(&path) {
                if bytes.len() <= MAX_SOURCE_BYTES
                    && String::from_utf8_lossy(&bytes).contains(snippet)
                {
                    candidates.push(path);
                }
            }
        } else {
            candidates.push(path);
        }
        if candidates.len() > 2 {
            break;
        }
    }
    match candidates.len() {
        1 => {
            let path = candidates.remove(0);
            let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_owned());
            let relative = path
                .strip_prefix(&canonical_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            Ok(ResolvedPath {
                state: MatchState::RelocatedWithEvidence,
                absolute: Some(path),
                relative: Some(relative),
                reason: if snippet.is_some() {
                    "The original path was unavailable; a unique filename plus reported snippet match supports this relocation.".to_owned()
                } else {
                    "The original path was unavailable and the report supplied no snippet; this relocation rests on a single filename match, which is evidence but not edit authority.".to_owned()
                },
                snippet_evidence: snippet.is_some(),
            })
        }
        count if count > 1 => Ok(ResolvedPath {
            state: MatchState::Ambiguous,
            absolute: None,
            relative: None,
            reason: "More than one candidate matched; CXView will not guess from a basename."
                .to_owned(),
            snippet_evidence: false,
        }),
        _ => Ok(ResolvedPath {
            state: MatchState::Unavailable,
            absolute: None,
            relative: None,
            reason:
                "The report path is unavailable and no unique snippet-backed relocation was found."
                    .to_owned(),
            snippet_evidence: false,
        }),
    }
}

pub struct ResolvedPath {
    pub state: MatchState,
    pub absolute: Option<PathBuf>,
    pub relative: Option<String>,
    pub reason: String,
    /// True when a relocation was supported by the reported snippet as well as a unique filename.
    /// A relocation that rests on a filename alone is readable evidence but not edit authority.
    pub snippet_evidence: bool,
}

pub fn load_code_context(
    root: &Path,
    finding: &FindingRecord,
    scan_prefix: Option<&str>,
    repository_prefix: Option<&str>,
) -> AppResult<CodeContext> {
    if finding.summary.category == FindingCategory::Sca {
        return Ok(CodeContext {
            finding_id: finding.summary.id.clone(),
            match_state: MatchState::NotApplicable,
            resolved_path: None,
            relative_path: None,
            current_source: None,
            current_range_start: None,
            current_range_end: None,
            reported_snippet: finding.reported_snippet.clone(),
            reported_nodes: finding.nodes.clone(),
            observed_local: vec![EvidenceStatement { label: EvidenceLabel::ObservedLocally, text: "This is a dependency finding; source navigation is supplemented by package manifest and lockfile evidence.".to_owned(), locator: None }],
            unknowns: vec!["Reachability is not inferred from a dev/prod label or partial lockfile evidence.".to_owned()],
            syntax: None,
            related_files: Vec::new(),
            source_hash: None,
            sca: Some(dependency_context(root, finding)?),
        });
    }
    if finding.summary.category == FindingCategory::Iac {
        return Ok(CodeContext {
            finding_id: finding.summary.id.clone(),
            match_state: MatchState::NotApplicable,
            resolved_path: None,
            relative_path: None,
            current_source: None,
            current_range_start: None,
            current_range_end: None,
            reported_snippet: finding.reported_snippet.clone(),
            reported_nodes: finding.nodes.clone(),
            observed_local: vec![EvidenceStatement {
                label: EvidenceLabel::Scanner,
                text: format!(
                    "IaC resource/rule evidence is available without deployment automation: {}.",
                    finding
                        .resource
                        .as_deref()
                        .unwrap_or("resource not supplied")
                ),
                locator: Some(finding.summary.raw_locator.clone()),
            }],
            unknowns: vec![
                "IaC provider behavior and deployment effects are not executed by CXView."
                    .to_owned(),
            ],
            syntax: None,
            related_files: Vec::new(),
            source_hash: None,
            sca: None,
        });
    }
    let resolved = resolve_finding_path(
        root,
        finding.summary.file_path.as_deref(),
        scan_prefix,
        repository_prefix,
        finding.reported_snippet.as_deref(),
    )?;
    let Some(path) = resolved.absolute.clone() else {
        return Ok(CodeContext {
            finding_id: finding.summary.id.clone(),
            match_state: resolved.state,
            resolved_path: None,
            relative_path: None,
            current_source: None,
            current_range_start: None,
            current_range_end: None,
            reported_snippet: finding.reported_snippet.clone(),
            reported_nodes: finding.nodes.clone(),
            observed_local: vec![EvidenceStatement {
                label: EvidenceLabel::ObservedLocally,
                text: resolved.reason,
                locator: None,
            }],
            unknowns: vec![
                "Current source is unavailable or ambiguous; no edit location is authorized."
                    .to_owned(),
            ],
            syntax: None,
            related_files: Vec::new(),
            source_hash: None,
            sca: None,
        });
    };
    let bytes = fs::read(&path).map_err(|source| AppError::Io {
        path: path.clone(),
        source,
    })?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(AppError::OversizedReport(MAX_SOURCE_BYTES as u64));
    }
    let source = String::from_utf8(bytes.clone()).map_err(|_| {
        AppError::Unsupported(
            "binary source files are read-only and cannot be proposed by CXView".to_owned(),
        )
    })?;
    let hash = sha256_hex(&bytes);
    let line = finding.summary.line_start.unwrap_or(1).max(1);
    let total_lines = source.lines().count().max(1) as u32;
    // A finding can report a line that no longer exists (the file shrank since the scan), so the
    // reported line is clamped before the window is derived instead of subtracted past zero.
    let line = line.min(total_lines);
    let range_start = line.saturating_sub(12).max(1);
    let range_end = (finding.summary.line_end.unwrap_or(line).saturating_add(12))
        .min(total_lines)
        .max(range_start);
    let current_source = source
        .lines()
        .skip(range_start.saturating_sub(1) as usize)
        .take((range_end - range_start + 1) as usize)
        .enumerate()
        .map(|(index, text)| format!("{:>5}  {}", range_start + index as u32, text))
        .collect::<Vec<_>>()
        .join("\n");
    let syntax = syntax_context(&path, &source);
    let mut observed_local = vec![EvidenceStatement {
        label: EvidenceLabel::ObservedLocally,
        text: format!(
            "Current bytes at {} are {} bytes with SHA-256 {}.",
            resolved.relative.as_deref().unwrap_or("<unknown>"),
            bytes.len(),
            hash
        ),
        locator: resolved.relative.clone(),
    }];
    if let Some(declaration) = syntax
        .as_ref()
        .and_then(|syntax| syntax.enclosing_declaration.clone())
    {
        observed_local.push(EvidenceStatement {
            label: EvidenceLabel::ObservedLocally,
            text: format!("The on-demand Oxc parse places the reported line inside {declaration}."),
            locator: resolved.relative.clone(),
        });
    }
    let mut unknowns = Vec::new();
    if finding.nodes.len() < 2 {
        unknowns.push("The report does not include a complete intermediate flow; endpoints are shown without inferring missing nodes.".to_owned());
    }
    if syntax
        .as_ref()
        .is_some_and(|syntax| syntax.syntax_error_count > 0)
    {
        unknowns.push("The current source has recoverable parser errors; syntax context is advisory, not proof of behavior.".to_owned());
    }
    if matches!(resolved.state, MatchState::CurrentFileDiffers) {
        unknowns.push("The reported snippet was not found in current bytes; the scan/source snapshot may have drifted.".to_owned());
    }
    Ok(CodeContext {
        finding_id: finding.summary.id.clone(),
        match_state: resolved.state,
        resolved_path: Some(path.to_string_lossy().into_owned()),
        relative_path: resolved.relative,
        current_source: Some(current_source),
        current_range_start: Some(range_start),
        current_range_end: Some(range_end),
        reported_snippet: finding.reported_snippet.clone(),
        reported_nodes: finding.nodes.clone(),
        observed_local,
        unknowns,
        syntax,
        related_files: related_files(path.parent().unwrap_or(root)),
        source_hash: Some(hash),
        sca: None,
    })
}

fn syntax_context(path: &Path, source: &str) -> Option<SyntaxContext> {
    let source_type = SourceType::from_path(path).ok()?;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let mut imports = Vec::new();
    for line in source.lines().take(120) {
        let trimmed = line.trim();
        if trimmed.starts_with("import ")
            || trimmed.starts_with("export ") && trimmed.contains(" from ")
            || trimmed.contains("require(")
        {
            imports.push(trimmed.to_owned());
        }
        if imports.len() >= 20 {
            break;
        }
    }
    let declaration = source.lines().enumerate().find_map(|(index, line)| {
        let number = index + 1;
        (line.trim_start().starts_with("function ")
            || line.trim_start().starts_with("class ")
            || line.contains("=>") && line.contains("const "))
        .then(|| format!("{} at line {}", line.trim(), number))
    });
    Some(SyntaxContext { parser: "Oxc parser".to_owned(), parser_version: "0.133.0".to_owned(), syntax_error_count: parsed.errors.len(), imports, enclosing_declaration: declaration, parser_note: "Parser offsets remain UTF-8 byte offsets; CXView displays line coordinates derived from the current UTF-8 source. Oxc does not establish security data flow.".to_owned() })
}

fn related_files(directory: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for name in [
        "package.json",
        "tsconfig.json",
        "vite.config.ts",
        "vite.config.js",
    ] {
        let path = directory.join(name);
        if path.is_file() {
            files.push(path.to_string_lossy().into_owned());
        }
    }
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.flatten().take(40) {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if path.is_file()
                && (name.ends_with(".test.ts")
                    || name.ends_with(".test.tsx")
                    || name.ends_with(".spec.ts")
                    || name.ends_with(".spec.tsx"))
            {
                files.push(path.to_string_lossy().into_owned());
            }
        }
    }
    files.truncate(20);
    files
}

pub fn dependency_context(root: &Path, finding: &FindingRecord) -> AppResult<DependencyContext> {
    let package_root = nearest_package_root(root, finding.summary.file_path.as_deref())
        .unwrap_or_else(|| root.to_owned());
    let package_json_path = package_root.join("package.json");
    let package_json = fs::read_to_string(&package_json_path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let package_name = finding.summary.package_name.clone();
    let mut package_json_evidence = Vec::new();
    let mut direct_dependency = None;
    let mut dependency_type = None;
    if let (Some(package_name), Some(package_json)) =
        (package_name.as_deref(), package_json.as_ref())
    {
        for (key, label) in [
            ("dependencies", "production"),
            ("devDependencies", "development"),
            ("optionalDependencies", "optional"),
            ("peerDependencies", "peer"),
        ] {
            if package_json
                .get(key)
                .and_then(Value::as_object)
                .and_then(|map| map.get(package_name))
                .is_some()
            {
                direct_dependency = Some(true);
                dependency_type = Some(label.to_owned());
                package_json_evidence.push(format!("package.json {key}.{package_name}"));
            }
        }
        if direct_dependency.is_none() {
            direct_dependency = Some(false);
        }
    }
    let (package_manager, support) = detect_package_manager(Some(&package_root));
    let mut resolved_instances = Vec::new();
    let mut ownership_paths = finding.dependency_paths.clone();
    let mut notes = Vec::new();
    if package_manager.as_deref() == Some("npm") && support.supported_lockfile {
        let lock_path =
            package_root.join(support.lockfile.as_deref().unwrap_or("package-lock.json"));
        let lock: Value =
            serde_json::from_str(&fs::read_to_string(&lock_path).map_err(|source| {
                AppError::Io {
                    path: lock_path.clone(),
                    source,
                }
            })?)?;
        let version = lock
            .get("lockfileVersion")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        if ![2, 3].contains(&version) {
            notes.push(format!(
                "npm lockfileVersion {version} is outside the supported v2/v3 parser."
            ));
        }
        if let Some(packages) = lock.get("packages").and_then(Value::as_object) {
            for (install_path, value) in packages {
                let Some(name) = package_name_from_install_path(install_path) else {
                    continue;
                };
                if package_name.as_deref() != Some(name.as_str()) {
                    continue;
                }
                let type_for_instance = if install_path == &format!("node_modules/{name}") {
                    dependency_type
                        .clone()
                        .or_else(|| Some("direct or root-resolved".to_owned()))
                } else {
                    Some("transitive or nested".to_owned())
                };
                resolved_instances.push(ResolvedDependency {
                    install_path: install_path.clone(),
                    version: value.get("version").and_then(as_string),
                    dependency_context: type_for_instance.clone(),
                    dependency_type: type_for_instance,
                });
            }
            for (parent_path, value) in packages {
                if let Some(dependencies) = value.get("dependencies").and_then(Value::as_object) {
                    if dependencies.contains_key(package_name.as_deref().unwrap_or_default()) {
                        ownership_paths.push(format!(
                            "{} -> {}",
                            if parent_path.is_empty() {
                                "<root>"
                            } else {
                                parent_path
                            },
                            package_name.as_deref().unwrap_or("<unknown>")
                        ));
                    }
                }
            }
        } else {
            notes.push(
                "The npm lockfile has no packages map; a complete dependency graph is unavailable."
                    .to_owned(),
            );
        }
    } else if package_manager.is_some() {
        notes.push("The package manager was detected, but this release does not claim to parse its lockfile graph. Use the reviewed inspection commands below.".to_owned());
    } else {
        notes.push("No package manager lockfile was detected.".to_owned());
    }
    if resolved_instances.is_empty() && package_name.is_some() {
        notes.push(
            "No matching installed package instance was found in the supported lockfile."
                .to_owned(),
        );
    }
    let package_name_for_cmd = package_name
        .clone()
        .unwrap_or_else(|| "<package>".to_owned());
    let inspection_commands = match package_manager.as_deref() {
        Some("npm") => vec![
            format!("npm explain {package_name_for_cmd}"),
            "npm ls --all --package-lock-only".to_owned(),
        ],
        Some("pnpm") => vec![
            format!("pnpm why {package_name_for_cmd}"),
            "pnpm list --depth Infinity".to_owned(),
        ],
        Some("yarn") => vec![
            format!("yarn why {package_name_for_cmd}"),
            "yarn list --pattern <package>".to_owned(),
        ],
        Some("bun") => vec![format!("bun why {package_name_for_cmd}")],
        _ => Vec::new(),
    };
    Ok(DependencyContext {
        package_name,
        scan_version: finding.summary.scan_package_version.clone(),
        resolved_instances,
        direct_dependency,
        ownership_paths,
        package_json_evidence,
        inspection_commands,
        notes,
    })
}

fn package_name_from_install_path(path: &str) -> Option<String> {
    let marker = "node_modules/";
    let start = path.rfind(marker)? + marker.len();
    let rest = &path[start..];
    let mut parts = rest.split('/');
    let first = parts.next()?.to_owned();
    if first.starts_with('@') {
        Some(format!("{}/{}", first, parts.next()?))
    } else {
        Some(first)
    }
}

pub fn nearest_package_root(root: &Path, report_path: Option<&str>) -> Option<PathBuf> {
    let canonical_root = root.canonicalize().ok()?;
    let start = report_path
        .and_then(|path| {
            let normalized = path.replace('\\', "/");
            let candidate = Path::new(&normalized);
            if candidate.is_absolute()
                || normalized.as_bytes().get(1) == Some(&b':')
                || normalized.split('/').any(|part| part == "..")
            {
                return None;
            }
            let path = canonical_root.join(candidate);
            let canonical = path.canonicalize().ok()?;
            canonical.starts_with(&canonical_root).then_some(canonical)
        })
        .and_then(|path| path.parent().map(Path::to_owned))
        .unwrap_or_else(|| canonical_root.clone());
    let mut current = if start.is_file() {
        start.parent().unwrap_or(&start).to_owned()
    } else {
        start
    };
    loop {
        if current.join("package.json").is_file() {
            return Some(current);
        }
        // The search stops at the bound repository: a package root above it is outside the
        // folder the user authorized, and its scripts must not be offered as local checks.
        if current == canonical_root || !current.pop() || !current.starts_with(&canonical_root) {
            break;
        }
    }
    None
}

pub fn detect_package_manager(
    package_root: Option<&Path>,
) -> (Option<String>, PackageManagerSupport) {
    let Some(root) = package_root else {
        return (
            None,
            PackageManagerSupport {
                name: "unknown".to_owned(),
                lockfile: None,
                supported_lockfile: false,
                note: "No package root was found.".to_owned(),
            },
        );
    };
    let package_lock = root.join("package-lock.json");
    if package_lock.is_file() {
        let version = fs::read_to_string(&package_lock)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|json| json.get("lockfileVersion").and_then(Value::as_u64));
        let supported = matches!(version, Some(2 | 3));
        return (
            Some("npm".to_owned()),
            PackageManagerSupport {
                name: "npm".to_owned(),
                lockfile: Some("package-lock.json".to_owned()),
                supported_lockfile: supported,
                note: if supported {
                    format!("npm package-lock.json v{version:?} is statically supported.")
                } else {
                    format!(
                        "package-lock.json version {version:?} is not supported for complete graph parsing."
                    )
                },
            },
        );
    }
    if root.join("pnpm-lock.yaml").is_file() {
        return (Some("pnpm".to_owned()), PackageManagerSupport { name: "pnpm".to_owned(), lockfile: Some("pnpm-lock.yaml".to_owned()), supported_lockfile: false, note: "pnpm is detected; this release shows manifest evidence and commands but does not parse this lockfile.".to_owned() });
    }
    if root.join("yarn.lock").is_file() {
        return (Some("yarn".to_owned()), PackageManagerSupport { name: "yarn".to_owned(), lockfile: Some("yarn.lock".to_owned()), supported_lockfile: false, note: "Yarn is detected; this release shows manifest evidence and commands but does not parse this lockfile.".to_owned() });
    }
    if root.join("bun.lock").is_file() || root.join("bun.lockb").is_file() {
        return (Some("bun".to_owned()), PackageManagerSupport { name: "bun".to_owned(), lockfile: Some(if root.join("bun.lock").is_file() { "bun.lock" } else { "bun.lockb" }.to_owned()), supported_lockfile: false, note: "Bun is detected; this release shows manifest evidence and commands but does not parse this lockfile.".to_owned() });
    }
    (
        Some("npm".to_owned()),
        PackageManagerSupport {
            name: "npm".to_owned(),
            lockfile: None,
            supported_lockfile: false,
            note:
                "package.json exists but no lockfile was detected; resolved versions are unknown."
                    .to_owned(),
        },
    )
}

fn git_value(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn git_lines(cwd: &Path, args: &[&str]) -> Vec<String> {
    git_value(cwd, args)
        .map(|value| {
            value
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn normalize_report_path(path: &str) -> String {
    let mut value = path.replace('\\', "/");
    if value.len() >= 2 && value.as_bytes()[1] == b':' {
        value = value[2..].to_owned();
    }
    while value.starts_with('/') {
        value = value[1..].to_owned();
    }
    value
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

fn as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patching::capture_snapshot;
    use std::io::Write;

    #[test]
    fn exact_path_and_unique_snippet_relocation_are_distinct() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir_all(root.join("src")).unwrap();
        let mut file = fs::File::create(root.join("src/App.tsx")).unwrap();
        writeln!(
            file,
            "export function App() {{\n  return <main>safe</main>;\n}}"
        )
        .unwrap();
        let exact =
            resolve_finding_path(root, Some("src/App.tsx"), None, None, Some("safe")).unwrap();
        assert!(matches!(exact.state, MatchState::Matched));
        let relocated =
            resolve_finding_path(root, Some("old/App.tsx"), None, None, Some("safe")).unwrap();
        assert!(matches!(relocated.state, MatchState::RelocatedWithEvidence));
    }

    #[test]
    fn a_relocation_without_a_snippet_is_readable_but_not_editable() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir_all(root.join("moved")).unwrap();
        fs::write(root.join("moved/App.tsx"), "export const value = 1;\n").unwrap();

        // A unique filename with no reported snippet relocates for reading...
        let relocated = resolve_finding_path(root, Some("old/App.tsx"), None, None, None).unwrap();
        assert!(matches!(relocated.state, MatchState::RelocatedWithEvidence));
        assert!(!relocated.snippet_evidence);

        // ...but the reported snippet is what turns a relocation into edit authority.
        let with_snippet = resolve_finding_path(
            root,
            Some("old/App.tsx"),
            None,
            None,
            Some("export const value"),
        )
        .unwrap();
        assert!(with_snippet.snippet_evidence);

        let finding = sast_finding("old/App.tsx", Some(1));
        let refused = capture_snapshot(root, "task", "report", &finding, None, None);
        assert!(
            refused.is_err(),
            "a filename-only relocation must not authorize a snapshot"
        );
    }

    #[test]
    fn basename_without_evidence_is_not_an_edit_location() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir_all(root.join("a")).unwrap();
        fs::create_dir_all(root.join("b")).unwrap();
        fs::write(root.join("a/index.ts"), "a").unwrap();
        fs::write(root.join("b/index.ts"), "b").unwrap();
        let result =
            resolve_finding_path(root, Some("missing/index.ts"), None, None, None).unwrap();
        assert!(matches!(result.state, MatchState::Ambiguous));
    }

    #[test]
    fn a_package_root_above_the_bound_repository_is_not_offered() {
        let directory = tempfile::tempdir().unwrap();
        let outer = directory.path();
        fs::write(outer.join("package.json"), r#"{"scripts":{"test":"true"}}"#).unwrap();
        let bound = outer.join("bound");
        fs::create_dir_all(bound.join("nested")).unwrap();
        fs::write(bound.join("package.json"), r#"{"scripts":{"test":"true"}}"#).unwrap();

        // The bound repository has its own manifest, so it is used.
        let found = nearest_package_root(&bound, None).unwrap();
        assert_eq!(found.canonicalize().unwrap(), bound.canonicalize().unwrap());

        // A sub-folder without a manifest must not fall back to the manifest outside the
        // repository the user actually bound.
        assert_eq!(nearest_package_root(&bound.join("nested"), None), None);
    }

    fn sast_finding(path: &str, line_start: Option<u32>) -> FindingRecord {
        FindingRecord {
            summary: FindingSummary {
                id: "finding-sast".to_owned(),
                report_id: "report".to_owned(),
                fingerprint: "fp".to_owned(),
                engine: "SAST".to_owned(),
                scanner: Some("Checkmarx".to_owned()),
                rule: Some("CXSAST-RAW-HTML".to_owned()),
                title: "Untrusted data in raw HTML".to_owned(),
                severity: "High".to_owned(),
                original_severity: Some("High".to_owned()),
                result_status: None,
                triage_state: None,
                category: FindingCategory::Sast,
                file_path: Some(path.to_owned()),
                package_name: None,
                package_version: None,
                scan_package_version: None,
                line_start,
                line_end: None,
                evidence_readiness: EvidenceReadiness::Ready,
                local_task_state: TaskState::Investigating,
                raw_locator: "/scanResults/0/results/0".to_owned(),
            },
            description: None,
            recommendation: None,
            cwe: None,
            query_id: None,
            query_name: None,
            evidence_links: vec![],
            nodes: vec![],
            reported_snippet: None,
            advisory_aliases: vec![],
            ecosystem: None,
            affected_range: None,
            fixed_range: None,
            dependency_paths: vec![],
            reachability: None,
            resource: None,
            iac_rule: None,
            expected_value: None,
            actual_value: None,
            provider: None,
            context: None,
            raw: Value::Null,
        }
    }

    #[test]
    fn a_reported_line_past_the_end_of_the_file_is_clamped() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/App.tsx"), "one\ntwo\nthree\n").unwrap();
        let context = load_code_context(root, &sast_finding("src/App.tsx", Some(900)), None, None)
            .expect("a shrunken file must produce a bounded window, not a panic");
        assert_eq!(context.current_range_start, Some(1));
        assert_eq!(context.current_range_end, Some(3));
        assert!(context.current_source.unwrap_or_default().contains("three"));
    }

    #[test]
    fn npm_v3_fixture_preserves_multiple_versions_and_owners() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/repositories/sca-multiple-versions");
        let finding = FindingRecord {
            summary: FindingSummary {
                id: "finding-sca".to_owned(),
                report_id: "report".to_owned(),
                fingerprint: "fp".to_owned(),
                engine: "SCA".to_owned(),
                scanner: Some("Checkmarx".to_owned()),
                rule: Some("CVE-2021-23337".to_owned()),
                title: "lodash".to_owned(),
                severity: "Medium".to_owned(),
                original_severity: Some("Medium".to_owned()),
                result_status: None,
                triage_state: None,
                category: FindingCategory::Sca,
                file_path: None,
                package_name: Some("lodash".to_owned()),
                package_version: Some("4.17.20".to_owned()),
                scan_package_version: Some("4.17.20".to_owned()),
                line_start: None,
                line_end: None,
                evidence_readiness: EvidenceReadiness::Ready,
                local_task_state: TaskState::Investigating,
                raw_locator: "/scaScanResults/0/results/0".to_owned(),
            },
            description: None,
            recommendation: None,
            cwe: None,
            query_id: None,
            query_name: None,
            evidence_links: vec![],
            nodes: vec![],
            reported_snippet: None,
            advisory_aliases: vec![],
            ecosystem: Some("npm".to_owned()),
            affected_range: Some("<4.17.21".to_owned()),
            fixed_range: Some(">=4.17.21".to_owned()),
            dependency_paths: vec![],
            reachability: Some("unknown".to_owned()),
            resource: None,
            iac_rule: None,
            expected_value: None,
            actual_value: None,
            provider: None,
            context: None,
            raw: Value::Null,
        };
        let context = dependency_context(&root, &finding).unwrap();
        assert_eq!(context.resolved_instances.len(), 2);
        assert_eq!(context.direct_dependency, Some(false));
        assert!(
            context
                .ownership_paths
                .iter()
                .any(|path| path.contains("direct-package"))
        );
    }
}
