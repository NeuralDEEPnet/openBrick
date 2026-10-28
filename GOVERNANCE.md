# Governance

## Maintainers

At least two maintainers hold release rights at all times. Maintainers are listed in `MAINTAINERS.md` once the first community maintainer joins; until then Neural Deep Network Ltd engineers maintain the project. New maintainers are proposed by an existing maintainer and confirmed by the others after sustained, kind, high-quality contribution.

## Decisions

- Technical decisions are recorded as ADR rows in [docs/plan/03](docs/plan/03-tech-stack-decisions.md) with an id, alternatives, reasons and a "revisit when" trigger.
- Plan changes come as pull requests titled `plan: <topic>`.
- Disagreements are resolved by the maintainers after hearing the arguments; the founder breaks ties on product direction, the geometry lead on the Dimensional Standard.

## Keys and accounts

| Asset | Custodian | Backup |
|-------|-----------|--------|
| Tauri updater private key | release maintainer | offline copy held by a second maintainer |
| Code-signing identities (Apple, SignPath or Azure Artifact Signing) | company account owner | company password manager |
| Store accounts (Apple, Google, Microsoft, Flathub) | company account owner | second maintainer as admin |
| Package registries (crates.io, npm, PyPI) | release maintainer | second owner on each registry |

## Releases

Tagged from `main` by a maintainer after the release checklist passes (CI green on the full matrix, changelog, docs page, demo recorded for milestone releases). See [docs/plan/09](docs/plan/09-platforms-and-distribution.md).

## Code of conduct

Enforced by two maintainers per [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
