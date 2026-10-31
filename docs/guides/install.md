# Install the openbrick command line

The `openbrick` CLI builds brick-compatible parts into printable 3MF and STL
files, validates them and prints the calibration coupon
([print-test guide](print-test.md)). Each release on
[GitHub Releases](https://github.com/NeuralDEEPnet/openBrick/releases) has
prebuilt binaries for:

| System | Builds |
|--------|--------|
| Linux x86-64 and arm64 | glibc; a static musl build is the fallback for older systems |
| macOS | Apple silicon and Intel |
| Windows | x86-64 |

**The commands below work from the first release, v0.1.0, on.** Until that
release is published, [build from a clone](#from-a-clone).

## macOS and Linux

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/NeuralDEEPnet/openBrick/releases/latest/download/openbrick-installer.sh | sh
```

## Windows (PowerShell)

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/NeuralDEEPnet/openBrick/releases/latest/download/openbrick-installer.ps1 | iex"
```

Both installers pick the build for your system and put `openbrick` where
`cargo install` would: `$CARGO_HOME/bin`, or
`~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows). If that folder is not
on your `PATH` yet, they add it to your shell profile (Windows: your user
`PATH`) and say so; open a new terminal afterwards. Set
`OPENBRICK_NO_MODIFY_PATH=1` to leave your profile alone.

The shell installer also checks the archive's SHA-256 checksum, which is
written into it; the PowerShell installer that dist makes does not, so on
Windows you may prefer to [verify the download](#verify-a-download) first.
For one exact version, replace `latest/download` with `download/v0.1.0` (or
the version you want).

## With cargo-binstall

If you use [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), it
downloads the same release archives:

```sh
cargo binstall --git https://github.com/NeuralDEEPnet/openBrick openbrick
```

`--git` is needed until the crate is on crates.io (planned for M2,
[PACKAGING.md](../../PACKAGING.md)); from then on `cargo binstall openbrick` is
enough. Where no prebuilt archive fits your system, cargo-binstall compiles the
CLI from source instead. Third-party builds (QuickInstall) are switched off for
this crate.

## From a clone

You need Rust 1.90 or newer ([rustup.rs](https://rustup.rs)) and git. This also
works before the first release.

```sh
git clone https://github.com/NeuralDEEPnet/openBrick
cd openBrick
cargo install --path crates/openbrick --locked
```

## Check that it works

```sh
openbrick --version
openbrick about
```

`openbrick about` prints the non-affiliation disclaimer and the attribution.

## The example parts

The installers install the program only. The example part documents, such as
the logo tile in the [print-test guide](print-test.md), are in the
repository's `parts/` folder: clone the repository as above, or download
`source.tar.gz` from the release and unpack it. Then run the guide's commands
from that folder.

## Verify a download

Every file of a release (archives, installers, checksums and the SBOM) has a
[GitHub artifact attestation](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations):
a signed record, kept in Sigstore's public transparency log, that the file was
built by this repository's release workflow from a tagged commit. To check one
with the [GitHub CLI](https://cli.github.com) before you run it:

```sh
gh attestation verify openbrick-x86_64-unknown-linux-gnu.tar.xz --repo NeuralDEEPnet/openBrick
```

This works for the installer scripts too: download `openbrick-installer.sh`,
verify it, then run `sh openbrick-installer.sh`; it checks the archive it
downloads against the checksum written into it. On Windows, verify
`openbrick-x86_64-pc-windows-msvc.zip` itself and unpack `openbrick.exe` into a
folder on your `PATH`. Each archive also has a `.sha256` file, and
`sha256.sum` lists them all (`sha256sum -c sha256.sum --ignore-missing` checks
the ones you downloaded).

The binaries are not code-signed or notarised yet; that comes with the desktop
app ([docs/plan/09](../plan/09-platforms-and-distribution.md) section 3). The
attestation is how you check them meanwhile.

## What a release contains

| File | What it is |
|------|------------|
| `openbrick-<target>.tar.xz`, `openbrick-x86_64-pc-windows-msvc.zip` | the binary with `README.md`, `LICENSE`, `CHANGELOG.md` and `THIRD-PARTY-LICENSES.txt` (the licence texts of the crates compiled into it) |
| `openbrick-installer.sh`, `openbrick-installer.ps1` | the installers above |
| `openbrick.cdx.xml` | a [CycloneDX](https://cyclonedx.org) software bill of materials: the Rust crates compiled into the CLI, with versions and licences (as resolved for x86-64 Linux; the other builds differ in a few platform crates) |
| `sha256.sum`, `*.sha256` | checksums |
| `source.tar.gz` | the repository at the release tag |
| `dist-manifest.json` | a machine-readable list of all of the above |

The `<target>` names are `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`,
`x86_64-apple-darwin` and `aarch64-apple-darwin`.

## Update and uninstall

To update, run the installer again: it replaces the binary. To uninstall,
delete `openbrick` (`openbrick.exe`) from the folder above and the
installer's receipt, `~/.config/openbrick/` (Windows:
`%LOCALAPPDATA%\openbrick\`). A CLI installed with cargo-binstall or
`cargo install` is removed with `cargo uninstall openbrick`.

## Other channels

npm, uv and pipx, Homebrew and scoop follow in M2, desktop stores and winget
in M4 ([docs/plan/09](../plan/09-platforms-and-distribution.md) section 2,
[PACKAGING.md](../../PACKAGING.md)). Community packages (AUR, Nix and others)
are welcome; PACKAGING.md says how.
