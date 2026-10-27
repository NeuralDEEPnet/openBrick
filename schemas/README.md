# JSON Schemas

One schema per document kind, published with each release and validated in CI (`cargo xtask data` does structural checks in M0; full JSON Schema validation lands with the schemas in M1):

- `standard.schema.json` for `standards/dimensions-v1.json`
- `part.schema.json` for `openbrick/part@1` (`.obpart.json`)
- `process-profile.schema.json` and `fit-profile.schema.json` (`.obprofile.json`)
- `run.schema.json` (`.obrun.json`)
- `model.schema.json` for `model.json` inside `.obrick`

Schema versions are semver; minor versions are additive; majors ship a migration (docs/plan/08 section 9).
