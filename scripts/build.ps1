[CmdletBinding()]
param(
    [Alias("Targets")]
    [ValidateSet("all", "windows", "linux", "linux-gnu", "linux-musl")]
    [string[]]$Target = @("all"),
    [switch]$DryRun,
    [switch]$SkipInstall,
    [switch]$IncludeLite
)

$ErrorActionPreference = "Stop"

$ScriptRoot = Split-Path -Parent $PSCommandPath
$ReleaseScript = Join-Path $ScriptRoot "release.ps1"

if (-not (Test-Path $ReleaseScript)) {
    throw "Unable to find release script at $ReleaseScript"
}

$requestedTargets = New-Object System.Collections.Generic.HashSet[string] ([System.StringComparer]::OrdinalIgnoreCase)
foreach ($entry in $Target) {
    [void]$requestedTargets.Add($entry)
}

$buildWindows = $false
$buildLinuxGnu = $false
$buildLinuxMusl = $false

if ($requestedTargets.Contains("all")) {
    $buildWindows = $true
    $buildLinuxGnu = $true
    $buildLinuxMusl = $true
}
else {
    if ($requestedTargets.Contains("windows")) {
        $buildWindows = $true
    }

    if ($requestedTargets.Contains("linux")) {
        $buildLinuxGnu = $true
        $buildLinuxMusl = $true
    }

    if ($requestedTargets.Contains("linux-gnu")) {
        $buildLinuxGnu = $true
    }

    if ($requestedTargets.Contains("linux-musl")) {
        $buildLinuxMusl = $true
    }
}

if (-not ($buildWindows -or $buildLinuxGnu -or $buildLinuxMusl)) {
    throw "No build targets selected. Use -Target all, windows, linux, linux-gnu, or linux-musl."
}

$forwardedParams = @{}
if ($DryRun) {
    $forwardedParams.DryRun = $true
}

if ($SkipInstall) {
    $forwardedParams.SkipInstall = $true
}

if ($IncludeLite) {
    $forwardedParams.IncludeLite = $true
}

if (-not $buildWindows) {
    $forwardedParams.SkipWindows = $true
}

if (-not $buildLinuxGnu) {
    $forwardedParams.SkipLinuxGnu = $true
}

if (-not $buildLinuxMusl) {
    $forwardedParams.SkipLinuxMusl = $true
}

$selectedLabels = @()
if ($buildWindows) {
    $selectedLabels += "windows"
}

if ($buildLinuxGnu) {
    $selectedLabels += "linux-gnu"
}

if ($buildLinuxMusl) {
    $selectedLabels += "linux-musl"
}

Write-Host ("Selected targets: {0}" -f ($selectedLabels -join ", "))
& $ReleaseScript @forwardedParams