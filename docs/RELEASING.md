# Releasing

Releases work like Slat's and Glint's: GitHub Releases is the update feed, every asset is signed with minisign, and the app checks the signature against a key compiled into it before it installs anything.

## Cut a release

1. Bump `version` under `[workspace.package]` in the root `Cargo.toml`, and run `cargo check` so `Cargo.lock` follows.
2. Write `release-notes/vX.Y.Z.md`: one line on what the release is about, then bolded bullet points.
3. Optionally, check the Discover catalog: `cargo test -p brook-core catalog_feeds_are_live -- --ignored`.
4. Commit, then tag and push:

   ```sh
   git tag vX.Y.Z
   git push origin main vX.Y.Z
   ```

The `release` workflow refuses a tag that doesn't match `Cargo.toml` or has no notes. Then it:

- builds a universal (arm64 and x86_64) `Brook.app` on macOS, signs it ad hoc, and packs it as `Brook.zip` (first install) and `Brook-macos.app.tar.gz` (what the updater installs);
- builds `Brook-linux-x86_64.tar.gz` and `Brook-linux-aarch64.tar.gz` on Ubuntu 22.04, so they run on any distribution with glibc 2.35 or newer;
- signs every asset with `MINISIGN_SECRET_KEY`, with the trusted comment `Brook vX.Y.Z <asset>`;
- publishes the release with the notes.

## How updates are verified

`brook_core::update` asks `https://api.github.com/repos/dyascj/brook/releases/latest` for the newest release. It only takes a release that:

- is newer than the running version, compared numerically (`v0.10.0` beats `v0.9.9`; pre-release tags such as `v1.0.0-beta.1` never count);
- has this platform's asset and its `.minisig`;
- verifies against `PUBLIC_KEY` in `update.rs`, **and** carries exactly the trusted comment `Brook vX.Y.Z <asset>`. Because the version is inside the signed comment, an older signed build can't be passed off as a newer one.

Installing unpacks next to the running app, renames the running copy aside, renames the new one into place (putting the old one back if that fails), and relaunches once the old process exits.

## The signing key

- The public key is `PUBLIC_KEY` in `crates/brook-core/src/update.rs`.
- The secret key is the `MINISIGN_SECRET_KEY` repository secret. It was made with `minisign -G -W` (no password), so CI can sign without a prompt. Keep an offline copy; on the maintainer's Mac it lives at `~/.minisign/brook.key`.

If the key is ever lost or leaked, generate a new pair, replace `PUBLIC_KEY`, update the secret, and ship a release signed with the **old** key that contains the **new** public key. Installed copies take that release, and from then on trust the new key.

## Building locally

```sh
scripts/bundle-macos.sh                 # dist/Brook.app for this Mac's architecture
UNIVERSAL=1 scripts/bundle-macos.sh     # arm64 + x86_64 (needs both rustup targets)
scripts/bundle-linux.sh                 # dist/Brook-linux-<arch>.tar.gz
scripts/make-icons.sh                   # re-render icons after editing assets/brand/icon.svg
```
