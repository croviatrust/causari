# =============================================================
# Causari one-line installer (Windows / PowerShell)
#
# Usage:
#   iwr -useb https://causari.dev/install.ps1 | iex
#
# Optional:
#   $env:CAUSARI_VERSION = "v0.1.0"        # pin version
#   $env:CAUSARI_BIN_DIR = "C:\bin"        # custom install dir
# =============================================================
$ErrorActionPreference = 'Stop'

$Repo    = 'croviatrust/causari'
$Version = if ($env:CAUSARI_VERSION) { $env:CAUSARI_VERSION } else { $null }
$BinDir  = if ($env:CAUSARI_BIN_DIR) { $env:CAUSARI_BIN_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\causari' }

function Say($msg)  { Write-Host "causari: $msg" -ForegroundColor Cyan }
function Warn($msg) { Write-Host "causari: $msg" -ForegroundColor Yellow }
function Die($msg)  { Write-Host "causari: $msg" -ForegroundColor Red; exit 1 }

# ---- detect arch ----
$arch = if ([Environment]::Is64BitOperatingSystem) { 'x86_64' } else { Die 'only x86_64 Windows binaries are published — try `cargo install --git https://github.com/croviatrust/causari`' }
$target = "$arch-pc-windows-msvc"

# ---- resolve version ----
if (-not $Version) {
  $rel = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest" -UseBasicParsing
  $Version = $rel.tag_name
  if (-not $Version) { Die 'could not fetch latest release' }
}
Say "installing $Repo $Version ($target)"

# ---- download ----
$tmp = New-Item -ItemType Directory -Path (Join-Path $env:TEMP "causari-$([guid]::NewGuid())") -Force
$zip = Join-Path $tmp 're.zip'
$url = "https://github.com/$Repo/releases/download/$Version/re-$Version-$target.zip"
try {
  Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
} catch { Die "download failed: $url" }

# ---- verify sha256 (best effort) ----
try {
  $sumsUrl  = "https://github.com/$Repo/releases/download/$Version/SHA256SUMS.txt"
  $sumsFile = Join-Path $tmp 'SHA256SUMS.txt'
  Invoke-WebRequest -Uri $sumsUrl -OutFile $sumsFile -UseBasicParsing
  $expected = (Select-String "re-$Version-$target.zip" $sumsFile | Select-Object -First 1).Line.Split(' ')[0]
  $actual   = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
  if ($expected -and ($expected -ne $actual)) {
    Die "sha256 mismatch — refusing to install (expected $expected, got $actual)"
  }
  if ($expected) { Say 'sha256 verified' }
} catch { Warn "skipping sha256 verification ($_)" }

# ---- extract ----
Expand-Archive -Path $zip -DestinationPath $tmp -Force
$src = Join-Path $tmp 're.exe'
if (-not (Test-Path $src)) { Die 'extracted archive did not contain re.exe' }

# ---- install ----
New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
$dst = Join-Path $BinDir 're.exe'
Move-Item -Path $src -Destination $dst -Force
Say "installed $dst"

# ---- PATH ----
$userPath = [Environment]::GetEnvironmentVariable('PATH','User')
if ($userPath -notlike "*$BinDir*") {
  $newPath = if ($userPath) { "$userPath;$BinDir" } else { $BinDir }
  [Environment]::SetEnvironmentVariable('PATH', $newPath, 'User')
  Say "added $BinDir to your user PATH (open a new terminal to pick it up)"
}

# ---- cleanup ----
Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue

# ---- post ----
try { & $dst --version } catch { }
Say 'done. Run: re init'
