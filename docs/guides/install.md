# Install OpenBrick3D: the command line and the desktop app

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
release is published, [build from a clone](#from-a-clone). The desktop app
has installers of its own: see [Desktop app (alpha)](#desktop-app-alpha).

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
written into it, when the `sha256sum` command is there (most Linux systems
have it; macOS may only have `shasum`). Without `sha256sum` it prints
"skipping sha256 checksum verification" and installs anyway. The PowerShell
installer that dist makes checks no checksum at all. The check that works
everywhere is to [verify the download](#verify-a-download) with the GitHub CLI
first. For one exact version, replace `latest/download` with
`download/v0.1.0` (or the version you want).

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
repository's `parts/` folder. Get them from the same release as your
`openbrick`, because the part documents on the `main` branch can already need
a newer version of the CLI:

```sh
git clone --branch "v$(openbrick --version | cut -d' ' -f2)" https://github.com/NeuralDEEPnet/openBrick
cd openBrick
```

`openbrick --version` prints `openbrick 0.1.0`, so this clones the tag
`v0.1.0` (in PowerShell, type the tag yourself: `--branch v0.1.0`); git says
"You are in 'detached HEAD' state", which is fine for reading the parts. Or
download `source.tar.gz` from the same release and unpack it. Then run the
guide's commands from that folder. If you built the CLI from a clone, that
clone already matches.

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
| `openbrick.cdx.xml` | a [CycloneDX](https://cyclonedx.org) software bill of materials: the Rust crates that go into the CLI, with versions and licences. It errs on the side of listing too much: it is resolved for x86-64 Linux (the other builds differ in a few platform crates), it includes build-time helpers, and cargo-cyclonedx resolves features for the whole workspace, so it can name a crate or two that the CLI does not compile. `THIRD-PARTY-LICENSES.txt` follows the CLI's own dependency tree, for every platform |
| `sha256.sum`, `*.sha256` | checksums |
| `OpenBrick3D_<version>_universal.dmg`, `…_universal.app.tar.gz`, `…_x64-setup.exe`, `…_x64_en-US.msi`, `openbrick3d_<version>_amd64.deb`, `openbrick3d-<version>-1.x86_64.rpm` | the desktop app's installers, from the release after v0.1.0 on ([below](#desktop-app-alpha)); each carries the app's `LICENSE` and `THIRD-PARTY-LICENSES.txt` (the licence texts of the crates and of three.js compiled into the app) |
| `desktop-sha256.sum`, the installers' `*.sha256` | the desktop installers' checksums ([verify a desktop installer](#verify-a-desktop-installer)) |
| `source.tar.gz` | the repository at the release tag |
| `dist-manifest.json` | a machine-readable list of the CLI files above (not the desktop installers) |

The `<target>` names are `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`,
`x86_64-apple-darwin` and `aarch64-apple-darwin`.

## Update and uninstall

To update, run the installer again: it replaces the binary. To uninstall,
delete `openbrick` (`openbrick.exe`) from the folder above and the
installer's receipt, `~/.config/openbrick/` (Windows:
`%LOCALAPPDATA%\openbrick\`). A CLI installed with cargo-binstall or
`cargo install` is removed with `cargo uninstall openbrick`.

If the shell installer added `~/.cargo/bin` to your `PATH`, it did so the way
rustup does: it wrote `~/.cargo/env` and `~/.cargo/env.fish` (unless they were
there already); it added the line `. "$HOME/.cargo/env"` to `~/.profile` and
`~/.zshrc` (creating them if needed, or `~/.zshenv` if only that exists) and to
whichever of `~/.bashrc`, `~/.bash_profile` and `~/.bash_login` exist; and it
wrote `~/.config/fish/conf.d/openbrick.env.fish`, which you can delete. If you
use rustup, keep the `env` files and the profile lines, because rustup uses the
same ones; otherwise you can delete them too. On Windows, the PowerShell
installer added the folder to your user `PATH`; remove it in *Settings >
System > About > Advanced system settings > Environment Variables* if you
like.

## Desktop app (alpha)

The desktop app is the browser playground in its own window, with your
system's open and save dialogs ([apps/desktop](../../apps/desktop/README.md)).
From the release after v0.1.0 on, every release also carries its installers
(v0.1.0 has none; until the next release,
[build it from source](../../apps/desktop/README.md#build-and-run)):

| System | File | How to install |
|--------|------|----------------|
| macOS 13 or later, Apple silicon and Intel | `OpenBrick3D_<version>_universal.dmg` | open it and drag OpenBrick3D to Applications |
| | `OpenBrick3D_<version>_universal.app.tar.gz` | the same app as an archive: `tar -xzf` it into Applications |
| Windows 10 and 11, x86-64 | `OpenBrick3D_<version>_x64-setup.exe` | installs for your user, no administrator needed |
| | `OpenBrick3D_<version>_x64_en-US.msi` | the same app as an MSI, for managed computers |
| Linux x86-64 with glibc 2.39 or newer and WebKitGTK 4.1 (Ubuntu 24.04 or newer, Debian 13, Fedora 40 or newer) | `openbrick3d_<version>_amd64.deb` | `sudo apt install ./openbrick3d_<version>_amd64.deb` |
| | `openbrick3d-<version>-1.x86_64.rpm` | `sudo dnf install ./openbrick3d-<version>-1.x86_64.rpm` |

The Linux packages bring only the app and its licence texts (about 3.7 MiB);
the package manager adds the webview libraries from your distribution. They
need glibc 2.39 and say so (`libc6 (>= 2.39)`), so on an older system, such
as Ubuntu 22.04 or Debian 12, apt and dnf refuse to install them;
[build it from source](../../apps/desktop/README.md#build-and-run) there.
There is no AppImage yet: it would copy those libraries into the file, which
needs a licence decision first
(apps/desktop/README.md). On Linux, if the window stays blank (seen with some
NVIDIA drivers), start it with `openbrick-desktop --safe-rendering`.

### The first start of an unsigned app

The installers are not code-signed yet. Signing needs certificates from Apple
and for Windows, which come with M4
([docs/plan/09](../plan/09-platforms-and-distribution.md) section 3). Until
then your system cannot tell who made the app and asks you to confirm it once.
That is expected and nothing is wrong with your download; if you would rather
check it first, [verify it](#verify-a-desktop-installer). If you prefer not to
confirm an unsigned app at all, the CLI and the browser playground do the same
work.

**macOS.** The app is only ad-hoc signed (Apple silicon runs nothing without
a signature) and not notarised, so Gatekeeper stops it the first time
([Apple's guide](https://support.apple.com/guide/mac-help/mh40616/mac)):

- macOS 13 and 14: in Finder, Control-click (or right-click) OpenBrick3D in
  Applications, choose **Open**, then **Open** again in the dialog.
- macOS 15 and later: open the app once and close the message, then go to
  *System Settings > Privacy & Security*, scroll to *Security*, click
  **Open Anyway** next to the note about OpenBrick3D, confirm with your
  password, and choose **Open**.
- Or, in Terminal, remove the download mark from this one app:
  `xattr -dr com.apple.quarantine /Applications/OpenBrick3D.app`. This also
  helps if macOS says the app "is damaged and can't be opened": that is the
  same check, not a broken file.

macOS remembers your choice; later starts need nothing.

**Windows.** SmartScreen shows "Windows protected your PC" for the installer.
Click **More info**, then **Run anyway**. Your browser may also say the file
is not commonly downloaded; in Edge choose *… > Keep*, then *Show more >
Keep anyway*. Installing needs no other step, and the installed app starts
normally afterwards.

**Linux.** Nothing to confirm; the packages are not signed either, which apt
and dnf accept for a local file. If your dnf setup refuses unsigned local
packages, verify the file first and add `--nogpgcheck`.

### Verify a desktop installer

Each installer has a `.sha256` file, `desktop-sha256.sum` lists them all, and
every one has a GitHub artifact attestation, like the CLI files
([above](#verify-a-download)):

```sh
sha256sum -c desktop-sha256.sum --ignore-missing      # macOS: shasum -a 256 -c desktop-sha256.sum --ignore-missing
gh attestation verify OpenBrick3D_0.2.0_universal.dmg --repo NeuralDEEPnet/openBrick
```

### Update and uninstall the desktop app

There is no updater yet (M4): to update, install the new release over the old
one. To uninstall, drag OpenBrick3D from Applications to the Bin on macOS; on
Windows, use *Settings > Apps > Installed apps > OpenBrick3D > Uninstall*; on
Linux, `sudo apt remove openbrick3d` or `sudo dnf remove openbrick3d`.

## Other channels

npm, uv and pipx, Homebrew and scoop follow in M2, desktop stores and winget
in M4 ([docs/plan/09](../plan/09-platforms-and-distribution.md) section 2,
[PACKAGING.md](../../PACKAGING.md)). Community packages (AUR, Nix and others)
are welcome; PACKAGING.md says how.
