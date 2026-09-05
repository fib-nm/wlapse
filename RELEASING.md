# Releasing wlapse

Releases are created from annotated Git tags by `.github/workflows/release.yml`. The workflow accepts stable tags such as `v0.1.0` and prerelease tags such as `v0.2.0-rc.1`.

## One-time repository setup

In the GitHub repository settings, enable release immutability under **Settings → General → Releases**. This protects published tags and assets. The workflow supplies only the built-in `GITHUB_TOKEN`; no repository secret is required.

Also create an active tag ruleset under **Settings → Rules → Rulesets** for tags matching `v*`. Restrict tag creation, updates, and deletion, and allow bypass only for repository administrators who are authorized to publish releases. This prevents another account with ordinary write access from starting the privileged release workflow with an arbitrary tag.

## Prepare a release

Start from a clean, current `main` branch:

```sh
git switch main
git pull --ff-only origin main
git status --short
```

Choose a version without the `v` prefix:

```sh
VERSION=0.1.0
```

Set the same version in `Cargo.toml`. If it changed, update `Cargo.lock` and inspect the result:

```sh
cargo check
git diff -- Cargo.toml Cargo.lock
```

Run every local release gate:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
cargo test --locked
cargo build --release --locked --target x86_64-unknown-linux-gnu
curl --fail --location \
  --output appimagetool-x86_64.AppImage \
  https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
printf '%s  %s\n' \
  'ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0' \
  'appimagetool-x86_64.AppImage' | sha256sum --check
chmod +x appimagetool-x86_64.AppImage
curl --fail --location \
  --header 'Accept: application/octet-stream' \
  --output runtime-x86_64 \
  https://api.github.com/repos/AppImage/type2-runtime/releases/assets/456065460
printf '%s  %s\n' \
  '1cc49bcf1e2ccd593c379adb17c9f85a36d619088296504de95b1d06215aebbf' \
  'runtime-x86_64' | sha256sum --check
python3 scripts/prepare_release.py \
  --tag "v${VERSION}" \
  --target x86_64-unknown-linux-gnu \
  --binary target/x86_64-unknown-linux-gnu/release/wlapse \
  --output-dir dist
python3 scripts/build_appimage.py \
  --version "${VERSION}" \
  --binary target/x86_64-unknown-linux-gnu/release/wlapse \
  --appimagetool appimagetool-x86_64.AppImage \
  --runtime runtime-x86_64 \
  --output-dir dist
(cd dist && sha256sum ./*.tar.xz ./*.AppImage > SHA256SUMS)
(cd dist && sha256sum --check SHA256SUMS)
test "$(APPIMAGE_EXTRACT_AND_RUN=1 \
  "dist/wlapse-v${VERSION}-x86_64.AppImage" --version)" = \
  "wlapse ${VERSION}"
```

Commit and push the release version if there are version changes:

```sh
git add Cargo.toml Cargo.lock
git commit -m "Prepare release ${VERSION}"
git push origin main
```

Wait for the `CI` workflow on `main` to pass. Then tag that exact commit and push the tag:

```sh
git status --short
git tag -a "v${VERSION}" -m "wlapse ${VERSION}"
git push origin "v${VERSION}"
```

The tag push starts the `Release` workflow. It repeats all checks, verifies that the tag, `Cargo.toml`, and `wlapse --version` agree, creates a deterministic archive, an x86_64 AppImage, and `SHA256SUMS`, then publishes the GitHub Release. Tags with a prerelease suffix are automatically published as prereleases and are not marked Latest.

Monitor and inspect the result with GitHub CLI:

```sh
gh run list --workflow release.yml --limit 5
RUN_ID="$(gh run list --workflow release.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
gh run watch "${RUN_ID}" --exit-status
gh release view "v${VERSION}"
```

Do not move a published release tag or replace an asset. If a published release is wrong, fix the problem and publish a new patch version.
