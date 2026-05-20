[CmdletBinding()]
param(
    [ValidateRange(0, 65535)]
    [int]$SyncPort = 0,

    [ValidateRange(0, 65535)]
    [int]$AdminPort = 0,

    [switch]$SkipBuild,
    [switch]$KeepArtifacts
)

$ErrorActionPreference = "Stop"

$ScriptRoot = Split-Path -Parent $PSCommandPath
$ProjectRoot = (Resolve-Path (Join-Path $ScriptRoot "..")).Path
$BinaryPath = Join-Path $ProjectRoot "target\debug\floral-sync-server.exe"
$RunRoot = Join-Path $ProjectRoot (Join-Path "target" ("verify-restore-runtime-" + [Guid]::NewGuid().ToString("N")))
$ConfigPath = Join-Path $RunRoot "sync-server.toml"
$StdoutPath = Join-Path $RunRoot "server.stdout.log"
$StderrPath = Join-Path $RunRoot "server.stderr.log"

$SyncToken = "verify-restore-sync-token"
$AdminPassword = "hunter2"
$AdminSessionSecret = "verify-restore-admin-session-secret"

$serverProcess = $null
$succeeded = $false

function Invoke-Step {
    param(
        [string]$Description,
        [string]$CommandText,
        [scriptblock]$Action
    )

    Write-Host "==> $Description"
    Write-Host "    $CommandText"
    & $Action
}

function Assert-Condition {
    param(
        [bool]$Condition,
        [string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

function Read-LogFile {
    param([string]$Path)

    if (Test-Path $Path) {
        return Get-Content -Path $Path -Raw
    }

    return ""
}

function Get-FreeTcpPort {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    try {
        $listener.Start()
        return ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
    }
    finally {
        $listener.Stop()
    }
}

function Wait-ForServerReady {
    param(
        [string]$BaseSyncUrl,
        [hashtable]$Headers,
        [System.Diagnostics.Process]$Process,
        [string]$StdoutLog,
        [string]$StderrLog
    )

    $deadline = (Get-Date).AddSeconds(30)

    while ((Get-Date) -lt $deadline) {
        if ($Process.HasExited) {
            $stdout = Read-LogFile -Path $StdoutLog
            $stderr = Read-LogFile -Path $StderrLog
            throw "Server exited before becoming ready.`nSTDOUT:`n$stdout`nSTDERR:`n$stderr"
        }

        try {
            Invoke-RestMethod -Uri "$BaseSyncUrl/health" -Headers $Headers -TimeoutSec 2 | Out-Null
            return
        }
        catch {
            [System.Threading.Thread]::Sleep(250)
        }
    }

    $stdout = Read-LogFile -Path $StdoutLog
    $stderr = Read-LogFile -Path $StderrLog
    throw "Timed out waiting for server readiness.`nSTDOUT:`n$stdout`nSTDERR:`n$stderr"
}

try {
    if ($SyncPort -eq 0) {
        $SyncPort = Get-FreeTcpPort
    }

    if ($AdminPort -eq 0) {
        do {
            $AdminPort = Get-FreeTcpPort
        } while ($AdminPort -eq $SyncPort)
    }

    Assert-Condition ($SyncPort -ne $AdminPort) "Sync and admin ports must be different."

    Invoke-Step -Description "Prepare temporary verification workspace" -CommandText "New-Item -ItemType Directory -Force -Path `"$RunRoot`"" -Action {
        New-Item -ItemType Directory -Force -Path (Join-Path $RunRoot "data") | Out-Null
        New-Item -ItemType Directory -Force -Path (Join-Path $RunRoot "exports") | Out-Null
        New-Item -ItemType Directory -Force -Path (Join-Path $RunRoot "logs") | Out-Null
    }

    if (-not $SkipBuild) {
        Invoke-Step -Description "Build debug server" -CommandText "cargo build" -Action {
            cargo build
        }
    }
    elseif (-not (Test-Path $BinaryPath)) {
        throw "Expected debug binary at $BinaryPath. Run without -SkipBuild once or build the project first."
    }

    Invoke-Step -Description "Write temporary server config" -CommandText "Set-Content `"$ConfigPath`"" -Action {
        @"
sync_listen = ["127.0.0.1:$SyncPort"]
admin_listen = ["127.0.0.1:$AdminPort"]
db_path = "data/floral-sync.sqlite3"
export_dir = "exports"
log_path = "logs/floral-sync-server.log"
log_level = "info"
sync_token = "$SyncToken"
admin_session_secret = "$AdminSessionSecret"
"@ | Set-Content -Path $ConfigPath -Encoding UTF8
    }

    Invoke-Step -Description "Start temporary verification server" -CommandText "target/debug/floral-sync-server.exe --config `"$ConfigPath`"" -Action {
        $serverProcess = Start-Process -FilePath $BinaryPath `
            -ArgumentList @("--config", $ConfigPath) `
            -WorkingDirectory $ProjectRoot `
            -RedirectStandardOutput $StdoutPath `
            -RedirectStandardError $StderrPath `
            -PassThru
    }

    $baseSyncUrl = "http://127.0.0.1:$SyncPort"
    $baseAdminUrl = "http://127.0.0.1:$AdminPort"
    $syncHeaders = @{ Authorization = "Bearer $SyncToken" }
    $adminHeaders = @{ Origin = $baseAdminUrl }

    Invoke-Step -Description "Wait for sync API readiness" -CommandText "GET $baseSyncUrl/health" -Action {
        Wait-ForServerReady -BaseSyncUrl $baseSyncUrl -Headers $syncHeaders -Process $serverProcess -StdoutLog $StdoutPath -StderrLog $StderrPath
    }

    $bootstrapBody = @{ password = $AdminPassword } | ConvertTo-Json -Compress
    $createBody = @{
        deviceId = "device-a"
        changes = @(
            @{
                id = "note-1"
                title = "test-1"
                content = "created before backup"
                category = "Inbox"
                createdAt = "2026-05-20T10:00:00Z"
                updatedAt = "2026-05-20T10:00:00Z"
                deletedAt = $null
                contentHash = "note-1:v1"
                deviceId = "device-a"
            }
        )
    } | ConvertTo-Json -Depth 6 -Compress
    $deleteBody = @{
        deviceId = "device-a"
        changes = @(
            @{
                id = "note-1"
                title = "test-1"
                content = ""
                category = ""
                createdAt = "2026-05-20T10:00:00Z"
                updatedAt = "2026-05-20T10:10:00Z"
                deletedAt = "2026-05-20T10:10:00Z"
                contentHash = "note-1:deleted"
                deviceId = "device-a"
            }
        )
    } | ConvertTo-Json -Depth 6 -Compress
    $note2Body = @{
        deviceId = "device-a"
        changes = @(
            @{
                id = "note-2"
                title = "later-note"
                content = "created after backup"
                category = "Inbox"
                createdAt = "2026-05-20T10:20:00Z"
                updatedAt = "2026-05-20T10:20:00Z"
                deletedAt = $null
                contentHash = "note-2:v1"
                deviceId = "device-a"
            }
        )
    } | ConvertTo-Json -Depth 6 -Compress

    Invoke-Step -Description "Run restore replay verification" -CommandText "bootstrap, push create/delete, backup, restore, verify incremental changes" -Action {
        Invoke-RestMethod -Uri "$baseAdminUrl/admin/api/bootstrap" -Method Post -Headers $adminHeaders -ContentType "application/json" -Body $bootstrapBody -SessionVariable adminSession | Out-Null

        $pushCreate = Invoke-RestMethod -Uri "$baseSyncUrl/v1/push" -Method Post -Headers $syncHeaders -ContentType "application/json" -Body $createBody
        $backup = Invoke-RestMethod -Uri "$baseAdminUrl/admin/api/maintenance/backup" -Method Post -Headers $adminHeaders -WebSession $adminSession
        $pushDelete = Invoke-RestMethod -Uri "$baseSyncUrl/v1/push" -Method Post -Headers $syncHeaders -ContentType "application/json" -Body $deleteBody

        $restoreBody = @{ fileName = $backup.fileName } | ConvertTo-Json -Compress
        Invoke-RestMethod -Uri "$baseAdminUrl/admin/api/maintenance/restore" -Method Post -Headers $adminHeaders -WebSession $adminSession -ContentType "application/json" -Body $restoreBody | Out-Null

        $healthAfterFirstRestore = Invoke-RestMethod -Uri "$baseSyncUrl/health" -Headers $syncHeaders
        $changesAfterFirstRestore = Invoke-RestMethod -Uri "$baseSyncUrl/v1/changes?since=$($pushDelete.revision)" -Headers $syncHeaders
        $noteAfterFirstRestore = Invoke-RestMethod -Uri "$baseAdminUrl/admin/api/notes/note-1" -WebSession $adminSession

        $firstRestoreChanges = @($changesAfterFirstRestore.changes)
        Assert-Condition ($pushCreate.revision -eq 1) "Expected create revision to be 1, got $($pushCreate.revision)."
        Assert-Condition ($pushDelete.revision -eq 2) "Expected delete revision to be 2, got $($pushDelete.revision)."
        Assert-Condition ($healthAfterFirstRestore.revision -gt $pushDelete.revision) "Expected restore to advance the server revision beyond the delete revision."
        Assert-Condition ($firstRestoreChanges.Count -eq 1) "Expected one replayed change after restore, got $($firstRestoreChanges.Count)."
        Assert-Condition ($firstRestoreChanges[0].note.id -eq "note-1") "Expected replayed note id to be note-1."
        Assert-Condition ($null -eq $firstRestoreChanges[0].note.deletedAt) "Expected restored note to be active, but it was deleted."
        Assert-Condition ($noteAfterFirstRestore.id -eq "note-1") "Expected note detail lookup to return note-1 after restore."
        Assert-Condition ($null -eq $noteAfterFirstRestore.deletedAt) "Expected note detail after restore to be active."

        $pushNote2 = Invoke-RestMethod -Uri "$baseSyncUrl/v1/push" -Method Post -Headers $syncHeaders -ContentType "application/json" -Body $note2Body
        Invoke-RestMethod -Uri "$baseAdminUrl/admin/api/maintenance/restore" -Method Post -Headers $adminHeaders -WebSession $adminSession -ContentType "application/json" -Body $restoreBody | Out-Null

        $changesAfterSecondRestore = Invoke-RestMethod -Uri "$baseSyncUrl/v1/changes?since=$($pushNote2.revision)" -Headers $syncHeaders
        $activeNotesAfterSecondRestore = Invoke-RestMethod -Uri "$baseAdminUrl/admin/api/notes?state=active&page=1&pageSize=50" -WebSession $adminSession

        $secondRestoreChanges = @($changesAfterSecondRestore.changes)
        $note1Change = $secondRestoreChanges | Where-Object { $_.note.id -eq "note-1" }
        $note2Change = $secondRestoreChanges | Where-Object { $_.note.id -eq "note-2" }
        $activeNoteIds = @($activeNotesAfterSecondRestore.notes | ForEach-Object { $_.id })

        Assert-Condition ($secondRestoreChanges.Count -eq 2) "Expected two replayed changes after the second restore, got $($secondRestoreChanges.Count)."
        Assert-Condition (($note1Change | Measure-Object).Count -eq 1) "Expected note-1 to be replayed on second restore."
        Assert-Condition (($note2Change | Measure-Object).Count -eq 1) "Expected note-2 tombstone on second restore."
        Assert-Condition ($null -eq $note1Change[0].note.deletedAt) "Expected note-1 to remain active after second restore."
        Assert-Condition ($null -ne $note2Change[0].note.deletedAt) "Expected note-2 to be emitted as tombstone after second restore."
        Assert-Condition ($activeNoteIds.Count -eq 1 -and $activeNoteIds[0] -eq "note-1") "Expected only note-1 to remain active after second restore."

        [ordered]@{
            artifactsRetained = [bool]$KeepArtifacts
            runtimeRoot = if ($KeepArtifacts) { $RunRoot } else { $null }
            scenarioOne = [ordered]@{
                createdRevision = $pushCreate.revision
                deletedRevision = $pushDelete.revision
                revisionAfterRestore = $healthAfterFirstRestore.revision
                replayedChangeCount = $firstRestoreChanges.Count
                replayedNote = [ordered]@{
                    id = $firstRestoreChanges[0].note.id
                    title = $firstRestoreChanges[0].note.title
                    deletedAt = $firstRestoreChanges[0].note.deletedAt
                    revision = $firstRestoreChanges[0].revision
                }
                noteDetailAfterRestore = [ordered]@{
                    id = $noteAfterFirstRestore.id
                    title = $noteAfterFirstRestore.title
                    deletedAt = $noteAfterFirstRestore.deletedAt
                    revision = $noteAfterFirstRestore.revision
                }
            }
            scenarioTwo = [ordered]@{
                note2RevisionBeforeRestore = $pushNote2.revision
                replayedChangeCount = $secondRestoreChanges.Count
                replayedChanges = @(
                    $secondRestoreChanges | ForEach-Object {
                        [ordered]@{
                            id = $_.note.id
                            deletedAt = $_.note.deletedAt
                            revision = $_.revision
                        }
                    }
                )
                activeNoteIdsAfterRestore = $activeNoteIds
            }
        } | ConvertTo-Json -Depth 6
    }

    $succeeded = $true
}
finally {
    if ($null -ne $serverProcess -and -not $serverProcess.HasExited) {
        Stop-Process -Id $serverProcess.Id -Force
    }

    if ($succeeded -and -not $KeepArtifacts) {
        Remove-Item -Path $RunRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    else {
        Write-Host "Verification artifacts preserved at $RunRoot"
    }
}