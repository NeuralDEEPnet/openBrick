# Install

<!-- cargo xtask docs serves this page as guides/install.md until docs/guides/install.md exists. -->

Release installers for the `openbrick` command line (shell, PowerShell, cargo-binstall) are on their way; this page will describe them.

Until then, install the CLI from a clone of the repository with Rust 1.90 or newer, as the print-test guide describes in [section 1, Install](print-test.md#1-install):

```sh
git clone https://github.com/NeuralDEEPnet/openBrick
cd openBrick
cargo install --path crates/openbrick --locked
openbrick --version
```

No installation at all is needed for the [browser playground](https://neuraldeepnet.github.io/openBrick/), which designs logo tiles and calibration coupons and downloads them as 3MF or STL.
