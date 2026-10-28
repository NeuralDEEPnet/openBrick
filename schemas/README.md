# JSON Schemas

One JSON Schema (draft 2020-12) per document kind, published with each release and checked in CI by `cargo xtask data` (docs/plan/08 sections 1 and 9). The schemas are CC0 like the rest of the data (LICENSE-POLICY.md).

| Schema | Validates | Read in Rust by |
|--------|-----------|-----------------|
| `standard.schema.json` | `standards/dimensions-*.json` (the Dimensional Standard) | `openbrick_units::Standard` |
| `coupon.schema.json` | `standards/coupon-*.json` (`kind: "coupon"`, the calibration coupon sweep of docs/plan/05 section 4) | `openbrick_print::CouponSpec` |
| `process-profile.schema.json` | `profiles/process/*.json` (`kind: "process"`); schema 1.1, which adds the optional `recommendedTubeWallMm` and `maxRemovedAreaPct` (validator rules `TUBE.WALL` and `FEATURE.MIN`) | `openbrick_print::ProcessProfile` |
| `fit-profile.schema.json` | `profiles/fit/*.json` (`kind: "fit"`); schema 1.1, which adds the optional `calibration.pocketSet` (the pocket set of the printed coupon, which `openbrick calibrate record` writes with the pocket cells) | `openbrick_print::FitProfile` |
| `part.schema.json` | `parts/*.obpart.json` (`openbrick/part@1`) | `openbrick_part::PartDoc` |
| `report.schema.json` | validator reports, the output of `openbrick validate --json` (docs/plan/05 section 5); no data files, checked against the expected reports of the validator fixtures in `crates/openbrick-print/tests/fixtures/validator/` | `openbrick_print::Report` |

Still to come, with their document kinds: `run.schema.json` (`.obrun.json`) and `model.schema.json` (`model.json` inside `.obrick`).

## What `cargo xtask data` checks

Every `.json` file in `standards/` must be claimed by exactly one schema (by its file-name prefix, `dimensions-` or `coupon-`), so a new file there never goes unchecked.

For every file in `standards/`, `profiles/process/`, `profiles/fit/` and `parts/`, and for every fixture in `fixtures/valid/`:

1. It passes its schema. Each failure is printed with its JSON pointer and schema location, for example `at /features/0/op: "body.tyle" is not one of [...] [schema #/$defs/feature/properties/op/enum]`.
2. The Rust reader in the table above loads it. Parts also go through `PartDoc::check` (unique feature ids, known ops, anchors defined before use), the standard through a check that row ids are unique, because `STANDARD.mm(id)` only ever finds the first row with an id, and the coupon through `CouponSpec::check` (unique cell ids, every `standardId` a row of the standard with a nominal, every `offsetField` set by one cell only, and `endMm` a whole number of steps after `startMm`).
3. What the reader writes back (`serde_json::to_value`) passes the schema too. The writers omit absent optional fields instead of writing `null`, keep the `credit` block, and write no empty `date`, so a document Rust writes is a document the schema accepts.

It also keeps the older checks (every data file has a pragma, its `kind` and a `credit`), meta-validates each schema against draft 2020-12, checks that the shared `$defs` copies are identical, that each `$defs/atThisMinor` matches the version its `$id` names and that each `$defs/knownFields` matches its schema (see below), and self-checks the fixtures. For the print pipeline it also checks that the expected report of every validator fixture passes `report.schema.json` and loads as `openbrick_print::Report`, that the schema's rule ids are exactly `openbrick_print::IMPLEMENTED_RULES` plus `PLANNED_RULES`, and that recording the first and the last value of every coupon cell (`openbrick_print::record_with`; the two pocket cells together, as one pocket of the `full` set) writes a fit profile `fit-profile.schema.json` accepts (`crates/xtask/src/validator.rs`).

### What the schemas and the readers can still disagree on

Steps 2 and 3 run only on the repository's data and fixtures. For a document from elsewhere, the schema is the contract, so these are the known differences:

- **The schema is stricter in many places, on purpose.** Bounds, enums, patterns, required `credit`, and no `null` for an absent optional field (`"on": null` loads in Rust but fails the schema; leave the field out instead).
- **Where the schema could be looser, the readers were changed to match.** JSON and JSON Schema do not tell `1` from `1.0` (draft 2020-12 counts `1.0` as an integer), so `couponVersion` (and the coupon's `version` and `copies`) is read as a whole number whichever way it is written, and the schema caps it at the `u32` maximum the reader can hold.
- **Duplicate keys in one object** are invisible to the schema (a parsed JSON value keeps the last one), while serde rejects them for fixed-field objects. Step 2 catches them in the repository.
- **Readers drop what they do not model yet.** Writing a document back loses `kind`, `presets` and `lock` on parts and `author`, `license` and `url` inside part `provenance`; `provenance` on the other kinds; `notes` and `tolerances` on the standard; and `minFeatureRule`, `horizontalHoleStrategies`, `tessellation` and `notes` on process profiles. The result still passes the schema, but it is not the same document; see the follow-ups below.

## How the schemas are built

- **Sources.** Each schema is derived from the real data files, the serde structs listed above and docs/plan/05 (profiles) and 06 (parts). Fields documented in the plan but not read by Rust yet are allowed and say so in their description (`presets` and `lock` on parts; `minFeatureRule`, `horizontalHoleStrategies`, `tessellation` and `notes` on process profiles; `notes` and `tolerances` on the standard).
- **Required fields** are the struct fields without `#[serde(default)]`, plus the repository rules: `credit` on every document (LICENSE-POLICY.md; the process profiles gained one) and `source` on every row of the standard (every geometry number cites its source).
- **Bounds** catch unit and entry mistakes, not design choices: lengths in process profiles are positive (chamfers may be zero), `maxOverhangDeg` is in (0, 90], fit offsets stay within plus or minus 1 mm (the coupon sweeps of docs/plan/05 section 4 reach at most 0.30 mm from nominal: horizontal Technic holes, 5.10 - 4.80), `xyScale` within 0.9 to 1.1, `couponVersion` from 1 to 4294967295, and chosen coupon cells are positive and keyed by the coupon rows (`stud`, `tube`, `pocketTubeOD`, `pocketRib`, `technicH`, `pin`, `axle`, `bar`, `clip`). No brick dimension is hard-coded in a schema; those live in the standard.
- **Enums** follow the code and the plan: `confidence` (verified, likely, inferred), the feature `op` list (equal to `openbrick_part::KNOWN_OPS`, enforced by a test in `crates/openbrick-part/tests/schema.rs`), parameter `type`, `unit` and `manipulator`, `colorRole` (body, accent, serial), hole strategies (polyhole, ellipse, teardrop) and fit `intent` (fits-genuine-bricks, fits-itself; open question Q05).
- **Ids.** Profile ids are lower-case words joined by hyphens (`fdm-0.4`). Part ids are a namespace, a dot and a name (`ndn.logo-tile`). Feature ids may use letters, digits, `_` and `-` but no dot, because anchors are written `id.face`. Parameter names may not use `-`, because expressions refer to them as `$name`.
- **Shared definitions.** `pragma`, `semver`, `spdx`, `credit`, `provenance` and `profileId` are repeated in each schema's `$defs` so every file stands alone (editors and code generators need no network). `cargo xtask data` fails if the copies differ, so edit them everywhere at once. `atThisMinor` is repeated too, but it follows each schema's own version: `cargo xtask data` checks that it matches exactly the pragmas 1.0 to 1.N of the schema whose `$id` names 1.N.
- **`$id`** is `https://openbrick3d.org/schemas/<major.minor>/<file>`. It names the schema; nothing fetches it, and the domain is not live yet.

## Unknown fields and later minor versions

docs/plan/08 section 9 says minor versions only add fields and stay readable by older minors. The Rust readers ignore fields they do not know (parameters and features keep them in a `#[serde(flatten)]` map). That is forward compatible, but it also means a misspelt optional field (`experimantal`) is silently dropped. The schemas handle both cases with the pragma:

- **A document at the schema's minor or an older one** (pragma `1.0` or `1.0.x` for a 1.0 schema; `1.0` or `1.1` for the 1.1 process-profile and fit-profile schemas) may use only the fields that schema names. Each schema has `"if": { "$ref": "#/$defs/atThisMinor" }, "then": { "$ref": "#/$defs/knownFields" }`; `knownFields` lists, object by object, the property names such a document may use, with `"additionalProperties": false`. A typo is an error at the pointer of the object that holds it.
- **A document of a later minor** (`1.1`, ...) may carry fields added since. The 1.0 schema still checks every field it knows and lets the others through, so a tool that only has the 1.0 schema can still validate and read a 1.1 document.
- **A document of an older minor may not use a field added since.** A 1.1 schema lists the fields it added in `$defs/since1_1` and, through `"allOf": [{ "$ref": "#/$defs/since1_1" }]`, refuses each of them in a 1.0 document (a `false` schema at that field), because the 1.0 schema a 1.0 tool ships would refuse the same document.
- **Open on purpose at every version:** parameters and features in parts (op-specific fields such as `"0.6mm"`, `"$size"` or `"profile.minFeature"` are strings the evaluator reads), `material` in fit profiles, and `print`, `export`, `snap` connectors, `presets[].params` and `lock` in parts. These are free-form `serde_json::Value` blocks or flatten maps in Rust; their documented fields are typed and the rest is allowed.

`knownFields` repeats property names, so `cargo xtask data` checks it against the schema: every object with fixed `properties` must be listed in `knownFields` with exactly those names, or be marked open with an explicit `additionalProperties`. Adding a field therefore means adding it in both places, in a new minor version of the schema.

## Fixtures

`fixtures/valid/` holds documents that must pass all three checks above, and `fixtures/invalid/` documents that must fail their schema. A fixture picks its schema by file-name prefix: `standard-`, `coupon-`, `process-`, `fit-` or `part-`. Each invalid fixture is a valid document with exactly one thing broken, named for that rule (`part-unknown-op.json`, `fit-missing-chosen-cells.json`, `fit-coupon-version-too-large.json`, `process-negative-min-feature.json`, `standard-bad-confidence.json`, ...). The valid `*-later-minor-*` fixtures show what a document of a later minor than its schema may add (`fit-later-minor-new-fields.json` is a 1.2 document, since the fit-profile schema is at 1.1); `fit-misspelled-offset.json` and `process-misspelled-optional-field.json` show the same additions failing at the schema's own minors, and `fit-1.0-with-pocket-set.json` and `process-1.0-with-1.1-field.json` show a 1.1 field failing in a 1.0 document.

`fixtures/expected-failures.json` records, for each invalid fixture, the JSON pointer, the schema keyword and a piece of the validator's message (the broken value or property name) that it breaks. Every error the fixture raises must match all three, so a fixture that also fails for some other reason, such as a second missing property, is reported.

The self-check runs in `cargo xtask data` and in the xtask unit tests (`cargo test -p xtask`), which also cover the rules JSON Schema cannot state (duplicate standard ids, the `knownFields` check) with one passing and one failing case each. It requires at least one valid and two invalid fixtures per schema.

To add a fixture: copy a valid one, change one thing, give it a name that says what is wrong, and add its pointer, keyword and message to `expected-failures.json`. `cargo xtask data` prints each failure as `at <JSON pointer>: <message> [schema #<keyword location>]`, which is where to copy them from.

## Follow-ups

- The Rust structs do not model `provenance` on profiles and the standard, nor the fields listed under "Readers drop what they do not model yet". Writers such as `openbrick calibrate record` (backlog item 11) should keep them before they rewrite files people edit by hand.
- `Credit` now lives in `openbrick-units` and is shared by all four document kinds; `Provenance` still lives in `openbrick-part` and lacks the `author`, `license` and `url` fields that LICENSE-POLICY.md and the schemas name.

## Versioning

Schema versions are semver and follow the document pragma `"openbrick": "1.x"`: minor versions only add optional fields and get a new `$id` (`.../schemas/1.1/...`), whose `atThisMinor` and `knownFields` then cover 1.0 and 1.1 documents; majors ship a migration and `openbrick migrate` (docs/plan/08 section 9). Each schema moves on its own: today `process-profile.schema.json` is at 1.1 (adding `recommendedTubeWallMm` and `maxRemovedAreaPct`), `fit-profile.schema.json` at 1.1 (adding `calibration.pocketSet`) and the others at 1.0. A document that uses a field of a newer minor declares that minor in its pragma (the bundled process profiles and the example fit profile say `"1.1"`, and `openbrick calibrate record` writes `"1.1"` when it writes `pocketSet`), so a tool that only has the older schema reads it leniently instead of refusing it; the newer schema refuses such a field in a document that declares an older minor (`$defs/since1_1`).
