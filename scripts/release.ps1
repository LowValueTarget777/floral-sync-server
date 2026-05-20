[CmdletBinding()]
param(
    [switch]$DryRun,
    [switch]$SkipWindows,
    [switch]$SkipLinuxGnu,
    [switch]$SkipLinuxMusl,
    [switch]$SkipInstall
)

$ErrorActionPreference = "Stop"

$ScriptRoot = Split-Path -Parent $PSCommandPath
$ProjectRoot = (Resolve-Path (Join-Path $ScriptRoot "..")).Path
$ManifestPath = Join-Path $ProjectRoot "Cargo.toml"
$AdminUiPath = Join-Path $ProjectRoot "admin-ui"
$TargetRoot = Join-Path $ProjectRoot "target"
$ArtifactDir = Join-Path $TargetRoot "release-artifacts"

function Resolve-NpmCommand {
    if ($DryRun) {
        return "npm.cmd"
    }

    foreach ($candidate in @("npm.cmd", "npm")) {
        if (Get-Command $candidate -ErrorAction SilentlyContinue) {
            return $candidate
        }
    }

    throw "npm is required. Install Node.js before running this script."
}

function Invoke-Step {
    param(
        [string]$Description,
        [string]$CommandText,
        [scriptblock]$Action
    )

    Write-Host "==> $Description"
    Write-Host "    $CommandText"
    if (-not $DryRun) {
        & $Action
    }
}

function Require-Command {
    param(
        [string]$CommandName,
        [string]$Message
    )

    if ($DryRun) {
        return
    }

    if (-not (Get-Command $CommandName -ErrorAction SilentlyContinue)) {
        throw $Message
    }
}

function Test-CargoZigbuild {
    if ($DryRun) {
        return $true
    }

    & cargo zigbuild --help *> $null
    return $LASTEXITCODE -eq 0
}

$NpmCommand = Resolve-NpmCommand

Require-Command -CommandName cargo -Message "cargo is required. Install Rust before running this script."
Require-Command -CommandName rustup -Message "rustup is required so the Linux release targets can be installed."
Require-Command -CommandName $NpmCommand -Message "npm is required. Install Node.js before running this script."

$buildLinuxTargets = (-not $SkipLinuxGnu) -or (-not $SkipLinuxMusl)

if ($buildLinuxTargets) {
    Require-Command -CommandName zig -Message "zig is required for cargo-zigbuild. Install Zig before running this script."
    if (-not (Test-CargoZigbuild)) {
        if ($SkipInstall) {
            throw "cargo-zigbuild is not installed. Run 'cargo install cargo-zigbuild --locked' or rerun without -SkipInstall."
        }

        Invoke-Step -Description "Install cargo-zigbuild" -CommandText "cargo install cargo-zigbuild --locked" -Action { cargo install cargo-zigbuild --locked }
    }
}

if (-not $SkipInstall) {
    Invoke-Step -Description "Install admin UI dependencies" -CommandText "$NpmCommand --prefix `"$AdminUiPath`" ci" -Action { & $NpmCommand --prefix $AdminUiPath ci }

    if ($buildLinuxTargets) {
        Invoke-Step -Description "Install Rust Linux targets" -CommandText "rustup target add x86_64-unknown-linux-gnu x86_64-unknown-linux-musl" -Action { rustup target add x86_64-unknown-linux-gnu x86_64-unknown-linux-musl }
    }
}

Invoke-Step -Description "Prepare release artifact directory" -CommandText "New-Item -ItemType Directory -Force -Path `"$ArtifactDir`"" -Action {
    New-Item -ItemType Directory -Force -Path $ArtifactDir | Out-Null
}

if (-not $SkipWindows) {
    Invoke-Step -Description "Build Windows release" -CommandText "cargo build --manifest-path `"$ManifestPath`" --release" -Action { cargo build --manifest-path $ManifestPath --release }

    $WindowsSource = Join-Path $TargetRoot "release\floral-sync-server.exe"
    $WindowsDest = Join-Path $ArtifactDir "floral-sync-server-x86_64-pc-windows-msvc.exe"
    Invoke-Step -Description "Collect Windows artifact" -CommandText "Copy-Item `"$WindowsSource`" `"$WindowsDest`" -Force" -Action { Copy-Item $WindowsSource $WindowsDest -Force }
}

if (-not $SkipLinuxGnu) {
    Invoke-Step -Description "Build Linux GNU release" -CommandText "cargo zigbuild --manifest-path `"$ManifestPath`" --target x86_64-unknown-linux-gnu --release" -Action { cargo zigbuild --manifest-path $ManifestPath --target x86_64-unknown-linux-gnu --release }

    $LinuxGnuSource = Join-Path $TargetRoot "x86_64-unknown-linux-gnu\release\floral-sync-server"
    $LinuxGnuDest = Join-Path $ArtifactDir "floral-sync-server-x86_64-linux-gnu"
    $LegacyLinuxGnuDest = Join-Path $ArtifactDir "floral-sync-server-x86_64-unknown-linux-gnu"
    Invoke-Step -Description "Remove legacy Linux GNU artifact name" -CommandText "Remove-Item `"$LegacyLinuxGnuDest`" -Force -ErrorAction SilentlyContinue" -Action { Remove-Item $LegacyLinuxGnuDest -Force -ErrorAction SilentlyContinue }
    Invoke-Step -Description "Collect Linux GNU artifact" -CommandText "Copy-Item `"$LinuxGnuSource`" `"$LinuxGnuDest`" -Force" -Action { Copy-Item $LinuxGnuSource $LinuxGnuDest -Force }
}

if (-not $SkipLinuxMusl) {
    Invoke-Step -Description "Build Linux musl release" -CommandText "cargo zigbuild --manifest-path `"$ManifestPath`" --target x86_64-unknown-linux-musl --release" -Action { cargo zigbuild --manifest-path $ManifestPath --target x86_64-unknown-linux-musl --release }

    $LinuxMuslSource = Join-Path $TargetRoot "x86_64-unknown-linux-musl\release\floral-sync-server"
    $LinuxMuslDest = Join-Path $ArtifactDir "floral-sync-server-x86_64-linux-musl"
    $LegacyLinuxMuslDest = Join-Path $ArtifactDir "floral-sync-server-x86_64-unknown-linux-musl"
    Invoke-Step -Description "Remove legacy Linux musl artifact name" -CommandText "Remove-Item `"$LegacyLinuxMuslDest`" -Force -ErrorAction SilentlyContinue" -Action { Remove-Item $LegacyLinuxMuslDest -Force -ErrorAction SilentlyContinue }
    Invoke-Step -Description "Collect Linux musl artifact" -CommandText "Copy-Item `"$LinuxMuslSource`" `"$LinuxMuslDest`" -Force" -Action { Copy-Item $LinuxMuslSource $LinuxMuslDest -Force }
}

Write-Host "Release artifacts are available under $ArtifactDir"