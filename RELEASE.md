# Release Guide

## Current version

**v0.1.0** (2026-06-03)

## Download

Pre-built Windows binaries are attached to each [GitHub Release](https://github.com/xqnode/codex-helper/releases):

| File | Description |
|------|-------------|
| `CodexHelper-0.1.0-win64.zip` | Portable build — unzip and run `codex-helper.exe` |
| `CodexHelper-0.1.0-Setup.exe` | Inno Setup installer (zh-CN) |

### Requirements

- Windows 10/11 (64-bit)
- [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) (usually pre-installed on Windows 11)
- [Codex CLI](https://github.com/openai/codex) or Codex Desktop
- API key from at least one supported provider

## Build from source

```powershell
# Release binary + ZIP
.\build-zip.bat

# Release binary + ZIP + installer (requires Inno Setup 6)
.\build-all.bat
```

Artifacts are written to `dist/`.

## Publish a new release

1. Bump `version` in `Cargo.toml` and update `CHANGELOG.md`.
2. Build artifacts: `.\build-all.bat`
3. Commit and tag:

   ```powershell
   git tag v0.1.1
   git push origin main --tags
   ```

4. Create GitHub Release and upload `dist/*.zip` and `dist/*Setup.exe`:

   ```powershell
   gh release create v0.1.1 dist/CodexHelper-0.1.1-win64.zip dist/CodexHelper-0.1.1-Setup.exe `
     --title "v0.1.1" `
     --notes-file CHANGELOG.md
   ```

## Versioning

This project follows [Semantic Versioning](https://semver.org/):

- **MAJOR** — incompatible config or proxy behavior changes
- **MINOR** — new providers, models, or features
- **PATCH** — bug fixes and small improvements
