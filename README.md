# CXView

CXView is a local-first Tauri desktop remediation workbench. Its primary path is:

`CXone JSON → repository binding → evidence-grounded investigation → proposal → reviewed diff → checked Git apply → local validation → retained evidence`

It is not a severity dashboard, a CWE explainer, or a prompt exporter. The renderer is React/TypeScript/Vite with CSS Modules and CodeMirror 6; the native layer is Rust/Tauri 2 with SQLite persistence. Reports are immutable app-data snapshots addressed by SHA-256. Repository reads, path containment, Git patching, process execution, and persistence stay in Rust.

## Run locally

```sh
pnpm install
pnpm dev                 # browser shell only; native commands are intentionally unavailable
pnpm tauri dev           # native desktop workflow
pnpm test                # TypeScript/UI tests
pnpm test:e2e:native     # Rust unit and disposable-repository integration tests
pnpm build               # TypeScript check + production renderer
pnpm tauri build         # local Tauri bundle
```

If pnpm reports an ignored `esbuild` build script on a new machine, approve that package with `pnpm approve-builds --all` and rerun the install/build. CXView does not download Node, Git, package managers, or provider tools for the inspected repository.

The current host produced:

- macOS ARM64 app: `src-tauri/target/release/bundle/macos/CXView.app`
- macOS ARM64 disk image: `src-tauri/target/release/bundle/dmg/CXView_0.1.0_aarch64.dmg`

The acceptance platform in the handoff is Windows 11 x64. This host is macOS ARM64, so no Windows installer is claimed. The Windows configuration uses Tauri's WebView2 bootstrapper mode; the default app is not a self-contained WebView2 runtime. The local macOS bundle is unsigned/not notarized.

## Supported report and dependency formats

The checked-in fixtures are synthetic and explicitly labeled. No user-supplied CXone export was present in the workspace, so the exact production schema is not claimed as verified.

| Shape | Scanner branches | Behavior | Coverage |
| --- | --- | --- | --- |
| Grouped object with `scanResults`, `scaScanResults`, and `iacScanResults` | SAST, SCA, IaC | Parses individual records, keeps ordered SAST nodes, dependency/advisory fields, IaC resource/value fields, unsupported items, and declared-count mismatches | `fixtures/synthetic/grouped-cxone.json`, importer fixture test |
| Result-oriented object with top-level `results` array | SAST, SCA, unknown | Preserves unknown engine/severity/status values and unsupported records | `fixtures/synthetic/result-oriented-cxone.json`, importer fixture test |
| Grouped object with null sections | no findings | Records null-section warnings without erasing prior state or crashing | `fixtures/synthetic/null-sections-cxone.json`, importer unit test |
| npm `package-lock.json` v2/v3 | SCA dependency context | Preserves multiple installed versions, direct/transitive ownership, paths, and package-manager inspection commands | `fixtures/repositories/sca-multiple-versions`, repository test |
| pnpm, Yarn, Bun lockfiles | SCA declared evidence | Detected and disclosed, but graph parsing is unsupported in this release | capability matrix in `fixtures/README.md` |
| SARIF, PDF, summary-only JSON | none | Not silently treated as a complete findings report; unsupported input remains diagnostic | unsupported-shape importer behavior |

## Safety and evidence rules

- Scanner evidence, current-code observations, user notes, and AI hypotheses are separate labels. Missing values remain unknown.
- Current source is read-only. Proposals use exact old/new text anchors and a task snapshot. Paths, base hashes, file encoding, overlap, symlink/reparse ancestors, `.git`, binary files, and new-file absence are checked natively.
- Approval applies one displayed patch transaction. Git uses `git apply --check` and applies without staging, committing, stashing, resetting, or cleaning. A bounded original-content/post-hash journal supports guarded undo and protects subsequent edits.
- Local checks are discovered from the actual package manifest, previewed with executable/arguments/cwd/write/network disclosures, approved per run, bounded, and recorded with exit code/output/duration. A passing check never changes scanner status.
- Later report comparison requires visible provenance and reports “still observed”, “explicitly reported fixed”, “no longer observed under comparable scope”, “newly observed”, or “incomparable”. Omission from a filtered or partial export is not a confirmed fix.
- Report content and repository strings are rendered as untrusted text. Raw JSON inspection is intentional and bounded. Core import/investigation/review has no automatic network request and does not require Checkmarx credentials.
- The Storage control reports retained app-data usage and can delete the active workspace's linked local report/task/note/patch/validation data after exact confirmation. It never deletes or resets repository files and does not claim forensic secure deletion.

The optional Codex adapter verifies the installed CLI's advertised structured-output/read-only flags. When both are present, the UI exposes an explicit proposal-only invocation using `codex exec --sandbox read-only` from a CXView provider-run directory with no repository write directory passed; it cannot apply patches or run validation. Saved authentication and provider/network policy remain owned by the CLI and are disclosed before invocation. External task export/import remains available. This host detected `codex-cli 0.145.0` with both flags, but no live provider proposal call was run, so authentication, schema output, cancellation, and provider-error handling remain unverified.

## Verification record

The following were run locally on September 16, 2026:

- `pnpm test`: 2 TypeScript tests passed.
- `pnpm build`: TypeScript check and Vite production build passed. The renderer bundle is approximately 820 kB minified; Vite reports a chunk-size warning. No performance target is claimed from this build.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all` and `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml`: 14 tests passed, including grouped/result/null imports, npm lockfile v3 ownership, exact/relocated/ambiguous paths, stale-base rejection, nested-worktree handling, real Git apply/undo with unrelated work and subsequent-edit protection, manifest-derived validation, and a real `node` process exit.
- `cargo tauri build`: passed and produced the macOS app and disk image above.

The disposable React fixture is `fixtures/repositories/react-remediation`. Its committed baseline test intentionally fails on the raw HTML sink; the current fixture working tree is left with the demonstrated reviewed text-rendering change, and the native run recorded two passing tests after that change. The native patch tests prove the checked apply/undo path, while the packaged Windows app, Windows junction behavior, native UI scaling/drag/drop/focus, live Codex invocation, cancellation/interrupt simulation, and 10,000-finding performance budgets remain unverified on this host. No CI, GitHub Actions, pipeline, or release automation is included.
