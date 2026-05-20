[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Image,
    [string[]]$Tag,
    [string[]]$Platform = @("linux/amd64"),
    [switch]$Latest,
    [switch]$Push,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$ScriptRoot = Split-Path -Parent $PSCommandPath
$ProjectRoot = (Resolve-Path (Join-Path $ScriptRoot "..")).Path
$ManifestPath = Join-Path $ProjectRoot "Cargo.toml"
$DockerfilePath = Join-Path $ProjectRoot "docker\Dockerfile"

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

Require-Command -CommandName docker -Message "docker is required. Install Docker before running this script."

if (-not $DryRun) {
    & docker buildx version *> $null
    if ($LASTEXITCODE -ne 0) {
        throw "docker buildx is required. Install Docker Buildx before running this script."
    }
}

$versionMatch = Select-String -Path $ManifestPath -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $versionMatch) {
    throw "Unable to read the package version from Cargo.toml."
}

$version = $versionMatch.Matches[0].Groups[1].Value
$tags = New-Object System.Collections.Generic.List[string]

if ($Tag -and $Tag.Count -gt 0) {
    foreach ($item in $Tag) {
        $trimmed = $item.Trim()
        if ($trimmed.Length -gt 0 -and -not $tags.Contains($trimmed)) {
            $tags.Add($trimmed)
        }
    }
}
else {
    $tags.Add($version)
}

if (($Latest -or -not ($Tag -and $Tag.Count -gt 0)) -and -not $tags.Contains("latest")) {
    $tags.Add("latest")
}

if (-not $Push -and $Platform.Count -ne 1) {
    throw "Local builds can only use a single platform. Pass -Push for multi-platform builds."
}

$platformValue = $Platform -join ","
$tagArgs = @()
foreach ($item in $tags) {
    $tagArgs += @("--tag", "$Image`:$item")
}

$modeArgs = if ($Push) { @("--push") } else { @("--load") }
$commandText = "docker buildx build --file `"$DockerfilePath`" --platform $platformValue $($tagArgs -join ' ') $($modeArgs -join ' ') `"$ProjectRoot`""
$buildArgs = @("buildx", "build", "--file", $DockerfilePath, "--platform", $platformValue) + $tagArgs + $modeArgs + @($ProjectRoot)

Invoke-Step -Description "Build Docker image" -CommandText $commandText -Action {
    & docker @buildArgs
    if ($LASTEXITCODE -ne 0) {
        throw "docker buildx build failed with exit code $LASTEXITCODE."
    }
}

$resultLabel = if ($Push) { "Published" } else { "Built" }
Write-Host "$resultLabel Docker image tags:"
foreach ($item in $tags) {
    Write-Host "  $Image`:$item"
}