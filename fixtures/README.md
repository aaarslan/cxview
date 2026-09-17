# CXView report fixtures

These are non-sensitive synthetic fixtures created for this repository because the implementation handoff did not include a user export. They are deliberately labeled synthetic and do not claim to be the user's exact Checkmarx One schema.

For a hands-on import → bind → review → apply → validate demo, see [`docs/WALKTHROUGH.md`](../docs/WALKTHROUGH.md).

| Fixture | Adapter branch | Covered evidence | Purpose |
| --- | --- | --- | --- |
| `synthetic/grouped-cxone.json` | `cxone-grouped` | SAST with two ordered nodes, SCA advisory fields, IaC resource/rule, a mismatched declared total, and an unsupported top-level branch | Grouped compatibility and evidence-preservation tests |
| `synthetic/result-oriented-cxone.json` | `cxone-results` | Result-oriented SAST plus SCA and unknown severity/engine record | Result-oriented adapter and unknown-value tests |
| `synthetic/null-sections-cxone.json` | `cxone-grouped` | Null scanner sections and unknown metadata state | Null/empty diagnostic safety |
| `repositories/react-remediation` | Local Node fixture with an inner disposable Git baseline | Raw HTML sink and a manifest-backed `node --test` check | End-to-end proposal/apply/validation demonstration; its working tree is intentionally left with the reviewed change |
| `repositories/sca-multiple-versions` | Local npm fixture | npm lockfile v3 with multiple installed versions and two ownership paths | SCA graph parser test fixture |

The fixture reports are not SARIF, PDF, or summary-only exports. Those formats remain explicitly unsupported by the first-release importer.
