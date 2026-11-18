# Desktop v0.2.0 release and signing

Desktop version is `0.2.0` in `desktop/package.json`, `desktop/src-tauri/tauri.conf.json` and `desktop/src-tauri/Cargo.toml`. CLI releases use `vX.Y.Z`; desktop releases use **`desktop-vX.Y.Z`**.

## Packages

- macOS Intel / Apple Silicon: DMG
- Windows x64: MSI and NSIS EXE
- Linux x64: DEB, RPM and AppImage

The `Desktop Cross-platform Release` workflow runs on a pushed desktop tag or manual dispatch with an **existing** desktop tag. Each runner builds the PyInstaller Python engine sidecar natively before Tauri bundles it. The pipeline runs Python unit tests and publishes checksums. All releases are **pre-releases** until platform signing and installation tests are verified.

## Required secrets for production signatures

| GitHub Actions secret | Description |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64 encoded Developer ID Application .p12 |
| `APPLE_CERTIFICATE_PASSWORD` | Password for the .p12 |
| `APPLE_SIGNING_IDENTITY` | Developer ID Application identity |
| `APPLE_ID` | Apple account for notarization |
| `APPLE_APP_SPECIFIC_PASSWORD` | Apple app-specific password |
| `APPLE_TEAM_ID` | Apple Developer team ID |
| `WINDOWS_CERT_PFX_BASE64` | Base64-encoded trusted Authenticode PFX |
| `WINDOWS_CERT_PASSWORD` | PFX password |
| `RELEASE_GPG_PRIVATE_KEY` | ASCII-armored private key used for checksum signatures |
| `RELEASE_GPG_PASSPHRASE` | Private-key passphrase |

Never commit these secrets. If credentials are missing, builds may succeed but are **unsigned**. Windows Authenticode signing is performed after Tauri packaging; the MSI/EXE signatures are checked with `signtool verify /pa`. For macOS, Tauri uses the Apple environment variables; validate the resulting code signature and notarization before describing the DMG as trusted. Linux GPG signs the checksum manifest, not the DEB/RPM packages themselves.

## Release

```bash
git checkout main
git pull --ff-only
git tag desktop-v0.2.0
git push origin desktop-v0.2.0
```

Do not tag until the workflow has been tested and the application has been installed on representative Windows, Linux and macOS machines. Release CI currently cannot substitute for installation, Gatekeeper/SmartScreen checks, Linux desktop integration checks or actual provider CLI tests. The Desktop Architect UI uses a separate Rust implementation; parity with the Python Architect refresh engine must be verified before claiming full feature parity.
