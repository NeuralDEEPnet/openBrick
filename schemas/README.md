# JSON Schemas

One JSON Schema (draft 2020-12) per document kind, published with each release and checked in CI by `cargo xtask data` (docs/plan/08 sections 1 and 9). The schemas are CC0 like the rest of the data (LICENSE-POLICY.md).

| Schema | Validates | Read in Rust by |
|--------|-----------|-----------------|
| `standard.schema.json` | `standards/*.json` (the Dimensional Standard) | `openbrick_units::Standard` |
| `process-profile.schema.json` | `profiles/process/*.json` (`kind: "process"`) | `openbrick_print::ProcessProfile` |
| `fit-profile.schema.json` | `profiles/fit/*.json` (`kind: "fit"`) | `openbrick_print::FitProfile` |
| `part.schema.json` | `parts/*.obpart.json` (`openbrick/part@1`) | `openbrick_part::PartDoc` |

Still to come, with their document kinds: `run.schema.json` (`.obrun.json`) and `model.schema.json` (`model.json` inside `.obrick`).

## How the schemas are built

- **Sources.** Each schema is derived from the real data files, the serde structs listed above and docs/plan/05 (profiles) and 06 (parts). Fields documented in the plan but not read by Rust yet are allowed and say so in their description (for example `presets` and `lock` on parts, `tessellation` and `horizontalHoleStrategies` on process profiles, `notes` and `tolerances` on the standard).
- **Required fields** are the struct fields without `#[serde(default)]`, plus the repository rules: `credit` on every standard, fit profile and part, and `source` on every row of the standard (every geometry number cites its source).
- **Unknown fields.** Objects that serde reads into fixed fields are closed (`additionalProperties: false`): serde would drop an unknown field without a word, so a typo such as `experimantal` would silently change nothing. Objects that serde keeps open are open in the schema too: `Param` and `Feature` in parts (their extras land in a `#[serde(flatten)]` map, and op-specific values such as `"0.6mm"`, `"$size"` or `"profile.minFeature"` are strings the evaluator reads), and the free-form `serde_json::Value` blocks (`material` in fit profiles, `print`, `export` and `lock` in parts), whose documented fields are typed and whose others are allowed. Adding a field to a closed object therefore means a minor schema version in the same pull request.
- **Bounds** catch unit and entry mistakes, not design choices: lengths in process profiles are positive (chamfers may be zero), `maxOverhangDeg` is in (0, 90], fit offsets stay within plus or minus 1 mm (coupon sweeps span about 0.3 mm), `xyScale` within 0.9 to 1.1, chosen coupon cells are positive and keyed by the coupon rows (`stud`, `tube`, `pocketTubeOD`, `pocketRib`, `technicH`, `pin`, `axle`, `bar`, `clip`). No brick dimension is hard-coded in a schema; those live in the standard.
- **Enums** follow the code and the plan: `confidence` (verified, likely, inferred), the feature `op` list (equal to `openbrick_part::KNOWN_OPS`, enforced by a test in `crates/openbrick-part/tests/schema.rs`), parameter `type`, `unit` and `manipulator`, `colorRole` (body, accent, serial), hole strategies (polyhole, ellipse, teardrop) and fit `intent` (fits-genuine-bricks, fits-itself; open question Q05).
- **Shared definitions.** `pragma`, `semver`, `spdx`, `credit`, `provenance` and `profileId` are repeated in each schema's `$defs` so every file stands alone (editors and code generators need no network). `cargo xtask data` fails if the copies differ, so edit them everywhere at once.
- **Ids.** `$id` is `https://openbrick3d.org/schemas/<major.minor>/<file>`. It names the schema; nothing fetches it, and the domain is not live yet.
- **Not covered by JSON Schema** and left to Rust: unique feature ids and anchors defined before use (`PartDoc::check`, run in CI through `openbrick check`), and cross-field rules such as `minFeatureMm = 2 x lineWidthMm` or offsets equal to chosen cell minus nominal (`openbrick-print` tests).

## Fixtures

`fixtures/valid/` holds documents that must pass and `fixtures/invalid/` documents that must fail. A fixture picks its schema by file-name prefix: `standard-`, `process-`, `fit-` or `part-`. Each invalid fixture is a valid document with exactly one thing broken, named for that rule (`part-unknown-op.json`, `fit-missing-chosen-cells.json`, `process-negative-min-feature.json`, `standard-bad-confidence.json`, ...). `fixtures/expected-failures.json` records, for each invalid fixture, the JSON pointer and schema keyword it breaks; the check requires every error the fixture raises to match that pair, so a fixture that fails for some other reason is reported.

The self-check runs in `cargo xtask data` and in the xtask unit tests (`cargo test -p xtask`). It also requires at least one valid and two invalid fixtures per schema. Integration tests in `openbrick-units`, `openbrick-print` and `openbrick-part` parse every valid fixture with the real serde structs, so the schemas are never looser than the code.

To add a fixture: copy a valid one, change one thing, give it a name that says what is wrong, and add its pointer and keyword to `expected-failures.json`. `cargo xtask data` prints each failure as `at <JSON pointer>: <message> [schema #<keyword location>]`, which is where to copy them from.

## Versioning

Schema versions are semver and follow the document pragma `"openbrick": "1.x"`: minor versions only add optional fields, majors ship a migration and `openbrick migrate` (docs/plan/08 section 9).
