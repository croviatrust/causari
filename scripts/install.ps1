# =============================================================
# Causari one-line installer (Windows / PowerShell)
#
# Usage:
#   iwr -useb https://causari.dev/install.ps1 | iex
#
# Optional:
#   $env:CAUSARI_VERSION = "v0.1.0"        # pin version
#   $env:CAUSARI_BIN_DIR = "C:\bin"        # custom install dir
#   $env:CAUSARI_SKIP_VERIFY = "1"         # bypass the sha256 check (not recommended)
#
# The binary's sha256 is verified against SHA256SUMS.txt published with each
# GitHub release. To review the code first, build from source instead:
#   cargo install --git https://github.com/croviatrust/causari
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

# ---- verify sha256 (required by default) ----
if ($env:CAUSARI_SKIP_VERIFY -eq '1') {
  Warn 'CAUSARI_SKIP_VERIFY=1 set — installing WITHOUT checksum verification'
} else {
  $sumsUrl  = "https://github.com/$Repo/releases/download/$Version/SHA256SUMS.txt"
  $sumsFile = Join-Path $tmp 'SHA256SUMS.txt'
  try { Invoke-WebRequest -Uri $sumsUrl -OutFile $sumsFile -UseBasicParsing }
  catch { Die "could not download SHA256SUMS.txt for $Version — refusing to install unverified (set `$env:CAUSARI_SKIP_VERIFY='1' to override, or build from source: cargo install --git https://github.com/$Repo)" }
  $line = Select-String "re-$Version-$target.zip" $sumsFile | Select-Object -First 1
  if (-not $line) { Die "no checksum for re-$Version-$target.zip in SHA256SUMS.txt — refusing to install unverified" }
  $expected = $line.Line.Split(' ')[0].ToLower()
  $actual   = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
  if ($expected -ne $actual) { Die "sha256 mismatch — refusing to install (expected $expected, got $actual)" }
  Say "sha256 verified ($actual)"
}

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
