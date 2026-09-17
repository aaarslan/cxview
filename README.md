<div align="center">
  <img src="src-tauri/icons/icon.svg" alt="CXView icon" width="112" height="112">
  <h1>CXView</h1>
  <p><strong>Security findings to reviewed, locally validated code changes.</strong></p>
  <p>Local-first · evidence-first · Tauri 2 · Rust · React · TypeScript</p>

  <p>
    <a href="docs/WALKTHROUGH.md">Walk through the demo</a> ·
    <a href="fixtures/README.md">Explore the fixtures</a>
  </p>
</div>

> [!IMPORTANT]
> CXView is an early, working release rather than a finished security platform. The core workflow and native safety checks are implemented and tested locally. The macOS ARM64 app is verified here; Windows packaging, production Checkmarx exports, live provider calls, and large-report performance still need dedicated validation.

## The idea

Security scanners are good at saying *what looked suspicious*. They are much less helpful at connecting that claim to the code that exists today, producing a change someone can review, and showing what happened after the change.

CXView is a small desktop workbench for that missing middle. It opens an exported Checkmarx One JSON report beside a local repository and keeps the chain visible:

```text
report → repository → evidence → proposal → diff review → Git apply → local check → later scan
```

The renderer is intentionally calm and inspectable. The native Rust layer owns report parsing, path containment, Git operations, process execution, and SQLite persistence. There is no cloud account, telemetry pipeline, embedded model, or background repository index.

## What you can do

- Import supported grouped or result-oriented Checkmarx-style JSON and retain diagnostics, raw locators, unsupported records, and declared-count mismatches.
- Bind a report to a local folder and see whether each finding is matched, relocated with evidence, ambiguous, unavailable, or affected by source drift.
- Inspect scanner claims, current source, reported flow nodes, syntax context, dependency ownership, unknowns, and a finding-specific offline playbook as separate evidence lanes.
- Create a manual proposal, import one from another tool, or optionally ask an installed Codex CLI for a schema-validated proposal. Proposals never write to the repository.
- Review an actual diff, approve one bounded transaction, apply it with Git preflight, and retain a guarded undo journal.
- Discover checks from the repository’s real `package.json`, preview command/cwd/network/write disclosures, approve each run, and retain exit/output/duration evidence.
- Reimport a later report and classify findings conservatively instead of treating scanner absence as proof of a fix.
- Keep workspace state, notes, proposals, patch history, and validation results in app data, with an explicit storage/deletion control.

## Five-minute start

### Prerequisites

- Node.js 20.19+ (or 22.12+) and pnpm. This workspace is exercised with pnpm 11.
- Rust 1.85+ with Cargo, plus the native prerequisites listed by [Tauri](https://v2.tauri.app/start/prerequisites/).
- Git for repository metadata, reviewed patch application, and the Git-backed parts of the demo.
- Optional: an installed `codex` CLI for proposal generation. CXView does not install it.

The Tauri CLI is a `devDependencies` entry (`@tauri-apps/cli`), so `pnpm install` is the only setup step. A global `cargo install tauri-cli` is not required and is not used by the scripts here.

### Install and run

```sh
git clone <your-fork-url>
cd cxview
pnpm install
pnpm tauri
```

### To build run

```sh
pnpm exec tauri build
```

`pnpm dev` starts the browser preview. It is useful for UI work, but native file selection, repository inspection, SQLite state, Git patching, validation, and the provider adapter are intentionally unavailable there, and every native action says so.

The `esbuild` install script is already allowed by `pnpm-workspace.yaml`. If pnpm blocks a build script for a new dependency, run `pnpm approve-builds` and review that specific package rather than approving the whole tree.

For the full guided flow, follow [the walkthrough](docs/WALKTHROUGH.md). It uses only the checked-in synthetic fixtures and does not need Checkmarx credentials.

## The product loop

1. **Import** — Open a JSON report or drop it on the native window. The original bytes are hashed and retained as an immutable snapshot.
2. **Bind** — Choose the repository that produced the report. Prefix mappings are explicit; basename guessing is never used as edit authorization.
3. **Explain** — Select a finding and compare the scanner claim with current read-only source. Missing evidence stays visibly unknown.
4. **Propose** — Start with the offline playbook, edit exact text anchors manually, import an external proposal, or use the optional proposal adapter.
5. **Review** — CXView recomputes a real diff from the captured snapshot and shows touched files, rationale, and unresolved risks.
6. **Apply** — A separate approval rechecks every base hash and applies without staging, committing, stashing, resetting, or cleaning.
7. **Validate** — Checks come from the actual repository manifest inside the folder you bound. Each command needs a separate approval, is bounded by a 15-minute timeout, and retains at most 96 KiB of output while still recording the real exit status.
8. **Compare** — Import a later report with visible provenance. A local pass and a scanner conclusion remain separate facts.

## Supported evidence

The importer is deliberately explicit. The fixtures are synthetic because no production export was supplied with the implementation brief.

| Input shape | Coverage | Fixture |
| --- | --- | --- |
| Grouped object with `scanResults`, `scaScanResults`, and `iacScanResults` | SAST nodes and locations, SCA package/advisory evidence, IaC resource/value evidence, unsupported branches, count mismatches | [`grouped-cxone.json`](fixtures/synthetic/grouped-cxone.json) |
| Result-oriented object with a top-level `results` array | SAST, SCA, unknown engines/severities, unsupported records | [`result-oriented-cxone.json`](fixtures/synthetic/result-oriented-cxone.json) |
| Grouped object with null scanner sections | Safe empty import with warnings; no silent discard | [`null-sections-cxone.json`](fixtures/synthetic/null-sections-cxone.json) |
| npm `package-lock.json` v2/v3 | Multiple installed versions, direct/transitive ownership, dependency paths | [`sca-multiple-versions`](fixtures/repositories/sca-multiple-versions) |

pnpm, Yarn, and Bun lockfiles are detected and disclosed, but their dependency graphs are not parsed in this release. SARIF, PDF, and summary-only exports are not silently treated as complete findings reports.

## A safety model you can inspect

CXView is opinionated about where uncertainty and authority live:

- **Evidence is labeled.** “Scanner claim”, “observed locally”, “user supplied”, and “AI hypothesis” are different things in the UI and data model.
- **Source is read-only by default.** A proposal buffer is not a live-file editor. Exact old/new anchors, expected absence, file hashes, encoding, path containment, symlink/reparse ancestors, and Git state are checked natively. A finding whose file could only be relocated by filename, without a matching snippet, stays readable but cannot authorize an edit.
- **Approval is narrow.** Reviewing a patch is not applying it. Applying a patch is not running a command. A passing command is not a scanner fix.
- **Git stays recoverable.** CXView uses `git apply --check`, applies without staging or committing, records post-hashes, and only undoes its own guarded journal when subsequent edits have not invalidated it.
- **Commands are disclosed.** Validation is discovered from the real manifest — never from a parent directory above the folder you bound — shows executable/arguments/cwd plus network and write notes, removes known provider-token variables, and records bounded output.
- **Providers are optional.** The Codex adapter is proposal-only, read-only, user initiated, and schema validated. The core import/investigation/review path makes no automatic network request.
- **Data stays local.** Reports and task bundles can contain proprietary code, paths, or secrets. Storage is inspectable and deletable from the app; CXView does not claim encryption or forensic secure deletion.

## Architecture

```text
React + TypeScript + CSS Modules + CodeMirror 6
                         │ Tauri invoke
                         ▼
Rust commands
  ├─ importer       versioned report adapters and raw locators
  ├─ repository     containment, Git metadata, source/syntax context
  ├─ patching       exact anchors, diff review, apply/undo journal
  ├─ validation     manifest-derived, approved local process runs
  ├─ provider       optional read-only Codex proposal adapter
  ├─ comparison     conservative later-report reconciliation
  └─ SQLite         profiles, snapshots, tasks, notes, patches, runs
```

Useful code landmarks:

| Path | Responsibility |
| --- | --- |
| `src/App.tsx` | Workspace state, workflow actions, dialogs, evidence presentation |
| `src/components/CodeSurface.tsx` | Lazy CodeMirror source and diff surfaces |
| `src/lib/api.ts` / `src/lib/types.ts` | Typed renderer/native boundary and shared contracts |
| `src/styles/` | Theme tokens, responsive layout, component styling |
| `src-tauri/src/commands.rs` | Tauri command surface and task orchestration |
| `src-tauri/src/importer.rs` | Report adapters, diagnostics, normalization, hashing |
| `src-tauri/src/repository.rs` | Safe path resolution and current-code context |
| `src-tauri/src/patching.rs` | Snapshot manifests, Git preflight, guarded undo |
| `src-tauri/src/validation.rs` | Check discovery and bounded process evidence |
| `src-tauri/src/db.rs` | SQLite schema and persistence |
| `fixtures/` | Synthetic reports and disposable repositories for tests/demo |

## Local quality gates

There is intentionally no CI configuration, GitHub Actions workflow, pipeline, release automation, or CI badge in this repository. The project keeps its quality gates runnable and inspectable on a contributor’s machine:

```sh
pnpm test
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
pnpm tauri build
```

`cargo test` runs the unit tests plus `src-tauri/tests/fixture_walkthrough.rs`, which replays the documented demo end to end against a disposable copy of the fixture: import, evidence, snapshot, exact-anchor proposal, Git preflight, apply, the repository's own check, and the guarded undo.

`pnpm tauri build` writes local artifacts under `src-tauri/target/release/bundle/`. On the verified macOS ARM64 host, it produces:

- `src-tauri/target/release/bundle/macos/CXView.app`
- `src-tauri/target/release/bundle/dmg/CXView_0.1.0_aarch64.dmg`

The bundle is local and unsigned/not notarized. Windows is the intended acceptance platform, but a Windows installer has not been claimed from this macOS host.

## Contributing

The best contributions make one part of the evidence chain more trustworthy without making the product more magical.

1. Start with a focused issue or a small branch that states the user-visible behavior and the safety boundary.
2. Add or update a synthetic fixture when changing report compatibility. Never commit a real export, credential, private source tree, or account data.
3. Keep unsupported and unknown values visible. Do not turn a missing field into a confident default.
4. For patching or process changes, add a test for stale input, scope/containment, approval, and failure behavior—not only the happy path.
5. Run the local quality gates above and describe what was actually exercised, including platform limitations.
6. Keep the renderer/native boundary narrow and avoid adding cloud services, telemetry, bundled developer tools, CI, or automatic writes.

Before publishing or accepting outside contributions, add a project license that reflects the maintainers’ intended terms. This repository does not invent a license on the maintainer’s behalf.

## Project boundaries

CXView is not:

- a Checkmarx replacement, scanner, or severity dashboard;
- a universal SARIF/PDF importer or a proof that a finding is exploitable/fixed;
- an automatic “fix every High” button, commit bot, PR bot, or deployment tool;
- a cloud agent platform, repository-wide index, telemetry product, or bundled model runtime.

The design record behind this implementation was supplied as a separate handoff document and is not part of this repository. What the product actually does is described by this README, by [`docs/WALKTHROUGH.md`](docs/WALKTHROUGH.md), and by the tests that run locally; nothing in the runtime depends on that document.
