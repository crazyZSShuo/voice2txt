# build.ps1 — VoiceInput build script
# Usage:
#   .\build.ps1 dev        # Hot-reload development mode
#   .\build.ps1 build      # Production build (.msi + .exe installer)
#   .\build.ps1 frontend   # Build frontend only
#   .\build.ps1 clean      # Remove all build artifacts

param(
    [Parameter(Position = 0)]
    [ValidateSet("dev", "build", "frontend", "clean")]
    [string]$Target = "dev"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ── Helpers ───────────────────────────────────────────────────────────────────

function Write-Step([string]$msg) {
    Write-Host "`n  → $msg" -ForegroundColor Cyan
}

function Write-Success([string]$msg) {
    Write-Host "  ✓ $msg" -ForegroundColor Green
}

function Write-Fail([string]$msg) {
    Write-Host "  ✗ $msg" -ForegroundColor Red
    exit 1
}

function Require-Command([string]$cmd) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
        Write-Fail "Required command not found: $cmd"
    }
}

function Ensure-NodeModules {
    if (-not (Test-Path "node_modules")) {
        Write-Step "Installing npm dependencies…"
        npm install
        if ($LASTEXITCODE -ne 0) { Write-Fail "npm install failed" }
    }
}

# ── Validate environment ──────────────────────────────────────────────────────

Require-Command "node"
Require-Command "npm"
Require-Command "cargo"

# ── Targets ───────────────────────────────────────────────────────────────────

switch ($Target) {

    # ── dev ──────────────────────────────────────────────────────────────────
    "dev" {
        Write-Host "`nVoiceInput — Development Mode" -ForegroundColor Magenta
        Ensure-NodeModules
        Write-Step "Starting Tauri dev server (hot reload)…"
        npx tauri dev
    }

    # ── build ────────────────────────────────────────────────────────────────
    "build" {
        Write-Host "`nVoiceInput — Production Build" -ForegroundColor Magenta
        Ensure-NodeModules

        Write-Step "Building frontend…"
        npm run build
        if ($LASTEXITCODE -ne 0) { Write-Fail "Frontend build failed" }
        Write-Success "Frontend built"

        Write-Step "Building Tauri application…"
        npx tauri build
        if ($LASTEXITCODE -ne 0) { Write-Fail "Tauri build failed" }
        Write-Success "Tauri build complete"

        # Report output locations
        $bundleDir = "src-tauri\target\release\bundle"
        Write-Host "`n  Output files:" -ForegroundColor Yellow

        $msi = Get-ChildItem -Path "$bundleDir\msi" -Filter "*.msi" -ErrorAction SilentlyContinue
        if ($msi) {
            foreach ($f in $msi) {
                $size = [math]::Round($f.Length / 1MB, 1)
                Write-Host "    MSI  : $($f.FullName) ($size MB)" -ForegroundColor White
            }
        }

        $exe = Get-ChildItem -Path "$bundleDir\nsis" -Filter "*.exe" -ErrorAction SilentlyContinue
        if ($exe) {
            foreach ($f in $exe) {
                $size = [math]::Round($f.Length / 1MB, 1)
                Write-Host "    NSIS : $($f.FullName) ($size MB)" -ForegroundColor White
            }
        }

        Write-Success "Build finished"
    }

    # ── frontend ─────────────────────────────────────────────────────────────
    "frontend" {
        Write-Host "`nVoiceInput — Frontend Build Only" -ForegroundColor Magenta
        Ensure-NodeModules

        Write-Step "Building React/TypeScript frontend…"
        npm run build
        if ($LASTEXITCODE -ne 0) { Write-Fail "Frontend build failed" }
        Write-Success "Frontend built → dist/"
    }

    # ── clean ─────────────────────────────────────────────────────────────────
    "clean" {
        Write-Host "`nVoiceInput — Cleaning Build Artifacts" -ForegroundColor Magenta

        $targets = @(
            "dist",
            "node_modules\.cache",
            "src-tauri\target"
        )

        foreach ($t in $targets) {
            if (Test-Path $t) {
                Write-Step "Removing $t…"
                Remove-Item -Recurse -Force $t
                Write-Success "Removed $t"
            } else {
                Write-Host "  (skip) $t — not found" -ForegroundColor DarkGray
            }
        }

        Write-Success "Clean complete"
    }
}
