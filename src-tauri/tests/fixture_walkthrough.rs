//! End-to-end check of the walkthrough in `docs/WALKTHROUGH.md`.
//!
//! It runs the documented loop against a disposable copy of the checked-in fixture: import the
//! synthetic report, read the SAST finding as evidence, capture the investigation snapshot, build
//! the patch from the walkthrough's exact anchors, apply it with the real Git preflight, run the
//! fixture's own `test` script, and undo the patch. Every step is asserted against the same
//! fixture and anchors the documentation gives a reader.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cxview_lib::importer::parse_report;
use cxview_lib::models::*;
use cxview_lib::patching::{apply_checked_patch, build_patch, capture_snapshot, undo_patch};
use cxview_lib::repository::{load_code_context, repository_root};
use cxview_lib::validation::{discover_checks, run_check};

const TASK_ID: &str = "task-walkthrough";
const TARGET_PATH: &str = "src/App.tsx";
const OLD_ANCHOR: &str = "return <section dangerouslySetInnerHTML={{ __html: value }} />;";
const NEW_TEXT: &str = "return <section>{value}</section>;";

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in walkdir::WalkDir::new(source) {
        let entry = entry.unwrap();
        let relative = entry.path().strip_prefix(source).unwrap();
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).unwrap();
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Copies the fixture repository and restores the vulnerable baseline the inner Git repository
/// records, so the documented replay starts from the state the walkthrough describes.
fn disposable_repository() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let copy = directory.path().join("react-remediation");
    copy_tree(&fixtures().join("repositories/react-remediation"), &copy);
    let restored = Command::new("git")
        .arg("-C")
        .arg(&copy)
        .args(["restore", "--source=HEAD", "--", TARGET_PATH])
        .status()
        .unwrap();
    assert!(
        restored.success(),
        "the fixture baseline could not be restored"
    );
    let root = repository_root(copy.to_str().unwrap()).unwrap();
    assert!(
        fs::read_to_string(root.join(TARGET_PATH))
            .unwrap()
            .contains("dangerouslySetInnerHTML"),
        "the replay must start from the vulnerable baseline"
    );
    (directory, root)
}

fn task_for(snapshot: SnapshotManifest, proposal: ProposalDocument) -> RemediationTask {
    RemediationTask {
        id: TASK_ID.to_owned(),
        profile_id: "profile-walkthrough".to_owned(),
        report_id: snapshot.report_id.clone(),
        finding_ids: vec!["finding-walkthrough".to_owned()],
        state: TaskState::ProposalReady,
        created_at: "now".to_owned(),
        updated_at: "now".to_owned(),
        snapshot,
        proposal: Some(proposal),
        diff: None,
        patch_id: None,
        notes: String::new(),
    }
}

#[test]
fn the_documented_fixture_walkthrough_holds() {
    let (_directory, root) = disposable_repository();

    // 1. Import the grouped report and read the diagnostics the walkthrough lists.
    let report_bytes = fs::read(fixtures().join("synthetic/grouped-cxone.json")).unwrap();
    let parsed = parse_report(
        "grouped-cxone.json",
        "fixtures/synthetic/grouped-cxone.json",
        report_bytes,
    )
    .unwrap();
    assert_eq!(parsed.diagnostics.adapter_id, "cxone-grouped");
    assert_eq!(parsed.diagnostics.adapter_version, "1.0.0");
    assert_eq!(parsed.diagnostics.parsed_by_engine.sast, 1);
    assert_eq!(parsed.diagnostics.parsed_by_engine.sca, 1);
    assert_eq!(parsed.diagnostics.parsed_by_engine.iac, 1);
    assert_eq!(parsed.diagnostics.unsupported_records.len(), 1);
    assert_eq!(parsed.diagnostics.count_mismatches.len(), 1);
    assert_eq!(parsed.diagnostics.count_mismatches[0].declared, 99);
    assert_eq!(
        parsed.report.metadata.project_id.as_deref(),
        Some("fixture-react")
    );
    assert_eq!(
        parsed.report.metadata.project_name.as_deref(),
        Some("react-remediation-fixture")
    );
    assert_eq!(parsed.report.metadata.branch.as_deref(), Some("main"));
    assert_eq!(
        parsed.report.metadata.completeness.as_deref(),
        Some("complete")
    );

    // 3. The finding the walkthrough asks the reader to select, read as evidence.
    let sast = parsed
        .findings
        .iter()
        .find(|finding| finding.summary.category == FindingCategory::Sast)
        .expect("the fixture contains one SAST finding");
    assert_eq!(sast.summary.raw_locator, "/scanResults/0/results/0");
    assert_eq!(sast.summary.file_path.as_deref(), Some(TARGET_PATH));
    assert_eq!(sast.summary.severity, "High");
    // Three reported steps: the importer records the result's own location as a node as well as
    // the two declared flow nodes, so the sink line appears both as the finding location and as
    // the declared sink. The walkthrough documents this as it is shown in the app.
    assert_eq!(sast.nodes.len(), 3);
    assert_eq!(
        sast.nodes
            .iter()
            .filter(|node| node.source_sink.as_deref() == Some("source"))
            .count(),
        1
    );
    assert_eq!(
        sast.nodes
            .iter()
            .filter(|node| node.source_sink.as_deref() == Some("sink"))
            .count(),
        1
    );
    assert!(
        sast.query_name
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains("raw html"),
        "the walkthrough identifies the finding by its raw-HTML title"
    );
    let context = load_code_context(&root, sast, None, None).unwrap();
    // The fixture's reported snippet is normalized (`{{__html: value}}`) while the file is not
    // (`{{ __html: value }}`), so the honest location state is "current file differs" and the
    // drift is listed under unknowns rather than being reported as a match.
    assert!(matches!(
        context.match_state,
        MatchState::CurrentFileDiffers
    ));
    assert!(
        context
            .unknowns
            .iter()
            .any(|unknown| unknown.contains("reported snippet was not found"))
    );
    assert!(
        context
            .current_source
            .as_deref()
            .unwrap_or_default()
            .contains("dangerouslySetInnerHTML")
    );

    // 4. Capture the investigation task snapshot.
    let snapshot = capture_snapshot(&root, TASK_ID, &parsed.report.id, sast, None, None).unwrap();
    assert_eq!(snapshot.files.len(), 1);
    assert_eq!(snapshot.files[0].path, TARGET_PATH);
    assert!(
        snapshot.files[0]
            .content
            .as_deref()
            .unwrap_or_default()
            .contains(OLD_ANCHOR)
    );

    // 5. Create the manual proposal with the walkthrough's exact anchors.
    let proposal = ProposalDocument {
        schema_version: "cxview-proposal-v1".to_owned(),
        task_id: TASK_ID.to_owned(),
        snapshot_id: TASK_ID.to_owned(),
        source: ProposalSource::Manual,
        provider: None,
        diagnosis: "This text-only preview does not need an HTML rendering sink.".to_owned(),
        assumptions: Vec::new(),
        edits: vec![TextEdit {
            path: TARGET_PATH.to_owned(),
            old_text: OLD_ANCHOR.to_owned(),
            new_text: NEW_TEXT.to_owned(),
            expected_absent: false,
        }],
        behavior_preservation: vec![
            "Preserve the component's visible text output without interpreting the value as markup."
                .to_owned(),
        ],
        suggested_tests: vec![
            "Run the fixture's actual test script and retain the result.".to_owned(),
        ],
        unresolved_questions: Vec::new(),
        evidence_refs: vec![sast.summary.raw_locator.clone()],
    };
    let task = task_for(snapshot, proposal.clone());
    let build = build_patch(&root, &task, &proposal).unwrap();
    assert_eq!(build.touched_files, vec![TARGET_PATH.to_owned()]);
    assert!(
        build
            .diff
            .contains("-  return <section dangerouslySetInnerHTML={{ __html: value }} />;")
    );

    // 6. Apply the reviewed patch and confirm the working tree changed without staging.
    let recovery = root.join("../recovery");
    let applied = apply_checked_patch(&root, &build, &recovery).unwrap();
    assert_eq!(applied.touched_files, vec![TARGET_PATH.to_owned()]);
    let applied_source = fs::read_to_string(root.join(TARGET_PATH)).unwrap();
    assert!(applied_source.contains(NEW_TEXT));
    assert!(!applied_source.contains("dangerouslySetInnerHTML"));

    // 7. Run the repository's own discovered check against the applied change.
    let candidates = discover_checks(&root).unwrap();
    let test_candidate = candidates
        .iter()
        .find(|candidate| candidate.id == "script-test")
        .expect("the fixture's package.json declares a test script");
    let run = run_check(&root, &task, &test_candidate.id, true).unwrap();
    assert!(
        matches!(run.status, ValidationStatus::Passed),
        "the fixture's own check should pass on the reviewed change: {}",
        run.note
    );
    assert_eq!(run.exit_code, Some(0));
    assert!(run.stdout.contains("pass 2"));

    // 8. Undo the patch through the journal and confirm the baseline is restored.
    let undone = undo_patch(&root, &build.journal).unwrap();
    assert_eq!(undone, vec![TARGET_PATH.to_owned()]);
    let restored_source = fs::read_to_string(root.join(TARGET_PATH)).unwrap();
    assert!(restored_source.contains(OLD_ANCHOR));
}
