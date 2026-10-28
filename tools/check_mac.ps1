# Type-check the macOS backend from Windows: cargo check --target aarch64-apple-darwin.
# Needs rustup (gnu host) + WinLibs MinGW. C helpers for the mac target are stubbed (no linking).
$ErrorActionPreference = "Continue"
$winlibs = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Directory -Filter "BrechtSanders.WinLibs*" | Select-Object -First 1
$env:PATH = "$env:USERPROFILE\.cargo\bin;$($winlibs.FullName)\mingw64\bin;$env:PATH"
$fake = Join-Path $PSScriptRoot "fakecc\fake_cc.cmd"
$env:CC_aarch64_apple_darwin = $fake
$env:AR_aarch64_apple_darwin = $fake
Set-Location (Join-Path $PSScriptRoot "..\src-tauri")
cargo check --target aarch64-apple-darwin --message-format short 2>&1 | ForEach-Object { "$_" }
