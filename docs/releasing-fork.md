# Releasing this fork

Releases and automatic updates belong to `jaimielovett/lumen`. The original project remains credited in the README and licence; dictionary downloads continue to use upstream's dictionary releases.

The fork's updater signing key is stored outside the repository at `~/.tauri/lumen-jaimielovett-updater.key`. Back up this file securely. Keep the same key for future releases, and never commit or upload it to GitHub. Its public key is in `src-tauri/tauri.conf.json`.

Every production frontend build checks the update feed and public signing key, including platform-specific Tauri overrides. A build fails if an upstream merge replaces either with upstream's update configuration. Review this check explicitly if the fork's signing identity ever needs to change.

## Prepare the version

Add bilingual release notes under `"next"` in `src/changelog.json`, then run:

```powershell
node scripts/release.mjs bump 1.1.0
node scripts/release.mjs stamp 1.1.0 2026-10-12
npm run build
git diff --check
```

Use the new version and date when making subsequent releases. Commit and push the release source before building the installer.

## Windows

The repository's **Windows** GitHub Actions workflow runs native Rust tests and builds an NSIS installer. It does not receive the private signing key. Run the workflow against the release commit, wait for success, then download its `lumen-windows` artifact.

```powershell
gh workflow run windows.yml --repo jaimielovett/lumen --ref main
# Replace RUN_ID with the completed run for the release commit.
gh run download RUN_ID --repo jaimielovett/lumen --name lumen-windows --dir Distribution/1.1.0
npx tauri signer sign --private-key-path "$env:USERPROFILE/.tauri/lumen-jaimielovett-updater.key" --password= --app-version 1.1.0 Distribution/1.1.0/Lumen_1.1.0_x64-setup.exe
node scripts/release.mjs manifest 1.1.0 "Tagalog and smoother reading" x86_64 jaimielovett/lumen Distribution/1.1.0/Lumen_1.1.0_x64-setup.exe
```

Create a release targeting the exact commit built by the workflow. Upload the installer, its `.sig`, and `src-tauri/target/release/bundle/release/latest.json`. Attach bilingual notes from `node scripts/release.mjs preview 1.1.0` and mark the release as latest. The signature is for the app's updater; it is not a Windows Authenticode signature.

## Mac

`Publier une version.command` builds and publishes from a Mac using the same fork signing key. The key must already be present on that Mac. It publishes only to this fork and does not modify upstream's release relay.
