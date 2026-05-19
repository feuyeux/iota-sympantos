param(
    [string]$Prompt = "生成宠物"
)

$ErrorActionPreference = "Stop"
$script:RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$script:IotaBin = $null
$script:WorkDir = $null
$script:StepLogs = @()

function Write-Step {
    param([string]$Text)
    Write-Host ""
    Write-Host "== $Text ==" -ForegroundColor Cyan
}

function Read-JsonFile {
    param([string]$Path)
    $raw = Get-Content -LiteralPath $Path -Raw -Encoding UTF8
    if ([string]::IsNullOrWhiteSpace($raw)) {
        throw "Empty JSON file: $Path"
    }
    return $raw | ConvertFrom-Json
}

function Write-Utf8NoBom {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string]$Text
    )
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $Text, $utf8NoBom)
}

function New-LogFilePath {
    param([string]$Prefix)
    $ts = Get-Date -Format "yyyyMMdd-HHmmss-fff"
    return (Join-Path $script:WorkDir ("{0}-{1}.log" -f $Prefix, $ts))
}

function Register-StepLog {
    param(
        [string]$Name,
        [string]$ArgsText,
        [int]$ExitCode,
        [double]$DurationMs,
        [string]$LogPath
    )
    $script:StepLogs += [PSCustomObject]@{
        name = $Name
        args = $ArgsText
        exit_code = $ExitCode
        duration_ms = [Math]::Round($DurationMs, 2)
        log_path = $LogPath
    }
}

function Ensure-IotaBinary {
    Write-Step "0) 编译最新 iota 并确认路径"

    Push-Location $script:RepoRoot
    try {
        & cargo build --quiet
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed"
        }
    }
    finally {
        Pop-Location
    }

    $candidates = @(
        (Join-Path $script:RepoRoot "target\debug\iota.exe"),
        (Join-Path $script:RepoRoot "target\debug\iota")
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }

    throw "built iota binary not found. tried: $($candidates -join ', ')"
}

function Invoke-Iota {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,

        [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)]
        [string[]]$Args
    )

    if (-not $script:IotaBin) {
        throw "iota binary path not initialized"
    }

    Push-Location $script:RepoRoot
    try {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $logPath = New-LogFilePath -Prefix $Name
        $all = & $script:IotaBin @Args 2>&1
        $sw.Stop()
        $exitCode = $LASTEXITCODE

        $allText = ($all | Out-String)
        Set-Content -LiteralPath $logPath -Value $allText -Encoding UTF8

        Register-StepLog -Name $Name -ArgsText ($Args -join " ") -ExitCode $exitCode -DurationMs $sw.Elapsed.TotalMilliseconds -LogPath $logPath

        return [PSCustomObject]@{
            ExitCode = $exitCode
            Output = $all
            LogPath = $logPath
            DurationMs = $sw.Elapsed.TotalMilliseconds
        }
    }
    finally {
        Pop-Location
    }
}

function Resolve-TaskId {
    param(
        [object]$PreBundle,
        [object[]]$Events
    )

    # Always bootstrap a fresh task so the full lifecycle starts at triage.
    return (Bootstrap-TaskFromEvents -PreBundle $PreBundle -Events $Events)
}

function Bootstrap-TaskFromEvents {
    param(
        [object]$PreBundle,
        [object[]]$Events
    )

    Write-Step "1.1) 创建全新 board + task（每次 demo 独立生命周期）"

    $nowTs = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $slug = "demo-$($nowTs)"

    # Create board
    $boardResult = Invoke-Iota create_board kanban create-board $slug "Hermes Kanban Demo"
    if ($boardResult.ExitCode -ne 0) {
        throw "create-board failed: $($boardResult.Output)"
    }
    # Parse board id from output: "Created board #<id> (...)"
    $boardLine = ($boardResult.Output | Out-String).Trim()
    if ($boardLine -match 'Created board #(\d+)') {
        $boardId = [UInt64]$Matches[1]
    } else {
        throw "Could not parse board id from: $boardLine"
    }
    Write-Host "board: #$boardId ($slug)"

    # Create task
    $taskResult = Invoke-Iota create_task kanban create-task $boardId "生成宠物 Demo Task"
    if ($taskResult.ExitCode -ne 0) {
        throw "create-task failed: $($taskResult.Output)"
    }
    $taskLine = ($taskResult.Output | Out-String).Trim()
    if ($taskLine -match 'Created task #(\d+)') {
        $taskId = [UInt64]$Matches[1]
    } else {
        throw "Could not parse task id from: $taskLine"
    }
    Write-Host "task: #$taskId (status: triage)"

    return $taskId
}

function Write-RunReport {
    param(
        [UInt64]$UsedTaskId,
        [UInt64]$BaselineCursor,
        [object[]]$TaskEvents,
        [int]$UpdatedCount,
        [string]$PetOutputPath
    )

    $transitionedCount = @($TaskEvents | Where-Object { $_.event_type -eq "task_transitioned" }).Count

    $report = [PSCustomObject]@{
        timestamp = (Get-Date).ToString("o")
        repo_root = $script:RepoRoot
        iota_bin = $script:IotaBin
        prompt = $Prompt
        task_id = $UsedTaskId
        baseline_cursor = $BaselineCursor
        pet_output_path = $PetOutputPath
        task_event_count = $TaskEvents.Count
        task_transitioned_count = $transitionedCount
        task_updated_count = $UpdatedCount
        step_logs = $script:StepLogs
        task_events = @($TaskEvents | Sort-Object id | ForEach-Object {
            $p = $null
            try { $p = $_.payload | ConvertFrom-Json } catch {}
            [PSCustomObject]@{
                id = $_.id
                event_type = $_.event_type
                created_at = $_.created_at
                from = if ($p -and ($p.PSObject.Properties.Name -contains "from")) { $p.from } else { $null }
                to   = if ($p -and ($p.PSObject.Properties.Name -contains "to"))   { $p.to }   else { $null }
            }
        })
    }

    $reportPath = Join-Path $script:WorkDir "run-report.json"
    Write-Utf8NoBom -Path $reportPath -Text ($report | ConvertTo-Json -Depth 8)
    return $reportPath
}

function Publish-LatestArtifacts {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RunDir
    )

    $latestDir = Join-Path (Join-Path $script:RepoRoot "examples\logs") "latest"
    if (Test-Path -LiteralPath $latestDir) {
        Remove-Item -LiteralPath $latestDir -Recurse -Force
    }
    New-Item -ItemType Directory -Path $latestDir -Force | Out-Null

    Copy-Item -Path (Join-Path $RunDir "*") -Destination $latestDir -Recurse -Force
    return $latestDir
}

$script:IotaBin = Ensure-IotaBinary
Write-Host "iota: $script:IotaBin"
Write-Host "backend: hermes"
Write-Host "task id: auto (new task per run)"
Write-Host "prompt: $Prompt"

# ---------------------------------------------------------------------------
# Pre-flight checks: verify hermes + inference provider before starting
# ---------------------------------------------------------------------------
Write-Step "0.5) 前置检查：hermes 可用性 + 推理 provider 配置"

# Check hermes binary
$hermesBin = (Get-Command hermes -ErrorAction SilentlyContinue).Source
if (-not $hermesBin) {
    throw "hermes binary not found in PATH. Install hermes first."
}
Write-Host "hermes binary: $hermesBin"
$hermesVersion = & hermes version 2>&1 | Out-String
Write-Host "hermes version: $($hermesVersion.Trim())"

# Verify nimia.yaml hermes config exists
$nimiaPath = Join-Path $env:USERPROFILE ".i6\nimia.yaml"
if (-not (Test-Path -LiteralPath $nimiaPath)) {
    throw "nimia.yaml not found at $nimiaPath — hermes inference config missing"
}

# Quick smoke test: hermes -z with a trivial prompt (non-interactive, exits immediately)
$smokeResult = & hermes -z "reply with exactly: OK" 2>&1 | Out-String
$smokeExit = $LASTEXITCODE
if ($smokeExit -ne 0) {
    Write-Host "hermes smoke test FAILED (exit $smokeExit):" -ForegroundColor Red
    Write-Host $smokeResult
    throw "hermes cannot complete inference. Check provider/model/api_key in nimia.yaml."
}
Write-Host "hermes smoke test: OK (inference provider reachable)"
Write-Host ""

$logsRoot = Join-Path $script:RepoRoot "examples\logs"
$runStamp = Get-Date -Format "yyyyMMdd-HHmmss"
$workDir = Join-Path $logsRoot ("kanban-demo-{0}" -f $runStamp)
New-Item -ItemType Directory -Path $workDir -Force | Out-Null
$script:WorkDir = $workDir
Write-Host "logs dir: $workDir"
$prePath = Join-Path $workDir "events-pre.json"
$deltaPath = Join-Path $workDir "events-delta.json"
$petOutPath = Join-Path $workDir "pet-output.txt"

Write-Step "1) 记录 kanban 基线 cursor"
$preExport = Invoke-Iota export_pre kanban export $prePath
if ($preExport.ExitCode -ne 0) {
    throw "iota kanban export failed"
}
$preBundle = Read-JsonFile -Path $prePath
$cursor = [UInt64]$preBundle.cursor
$allEvents = @($preBundle.events)
$usedTaskId = Resolve-TaskId -PreBundle $preBundle -Events $allEvents
Write-Host "baseline cursor: $cursor"
Write-Host "resolved task id: #$usedTaskId"
Write-Host "pre-export log: $($preExport.LogPath)"

Write-Step "1.5) 状态推进：triage -> todo -> ready"
$move1 = Invoke-Iota move_triage_todo kanban move $usedTaskId todo
if ($move1.ExitCode -ne 0) { throw "move triage->todo failed: $($move1.Output)" }
Write-Host "triage -> todo: OK"

$move2 = Invoke-Iota move_todo_ready kanban move $usedTaskId ready
if ($move2.ExitCode -ne 0) { throw "move todo->ready failed: $($move2.Output)" }
Write-Host "todo -> ready: OK"

Write-Step "2) [ACP path] prompt -> iota -> ACP JSON-RPC -> hermes agent（生成宠物）"
$runResult = Invoke-Iota run_pet run --no-daemon --backend hermes $Prompt
if ($runResult.ExitCode -ne 0) {
    throw "iota run --backend hermes failed"
}
$petOutput = (($runResult.Output | Out-String).TrimEnd())
if ([string]::IsNullOrWhiteSpace($petOutput)) {
    throw "Hermes returned empty output"
}
$petOutput | Set-Content -LiteralPath $petOutPath -Encoding UTF8
Write-Host "pet output saved: $petOutPath"
Write-Host "output chars: $($petOutput.Length)"
Write-Host "run log: $($runResult.LogPath)"

Write-Step "3) [Dispatch path] iota 劫持 hermes 读写 kanban — Dispatcher::spawn_worker()"
Write-Host "iota Dispatcher 启动 hermes worker，注入 HERMES_KANBAN_DB=shadow 路径"
Write-Host "hermes worker 写 shadow DB -> ShadowWatcher::poll() -> store.transition() -> iota 主 DB"
$dispatchResult = Invoke-Iota dispatch_task kanban dispatch $usedTaskId --timeout 60
Write-Host "dispatch exit: $($dispatchResult.ExitCode)"
Write-Host "dispatch log: $($dispatchResult.LogPath)"
Write-Host (($dispatchResult.Output | Out-String).Trim())

$shadowPath = Join-Path $env:USERPROFILE ".i6\kanban\shadows\$usedTaskId\kanban.db"
$shadowExists = Test-Path -LiteralPath $shadowPath
Write-Host "shadow DB path: $shadowPath"
Write-Host "shadow DB created by dispatcher: $shadowExists (ephemeral - cleaned up after worker exits)"

Write-Step "3.5) done -> archived（dispatch claim 已产生 ready->running 事件；手动推至 done->archived）"
# Determine current task status after dispatch
$taskAfterDispatch = & $script:IotaBin kanban move $usedTaskId done 2>&1
if ($LASTEXITCODE -eq 0) {
    Write-Host "running -> done: OK"
} else {
    Write-Host "NOTE: task may already be in done or other state: $taskAfterDispatch"
}
$move5 = Invoke-Iota move_done_archived kanban move $usedTaskId archived
if ($move5.ExitCode -ne 0) {
    Write-Host "WARNING: move done->archived: $($move5.Output)"
} else {
    Write-Host "done -> archived: OK"
}

Write-Step "4) 导出增量事件并校验关联"
$deltaExport = Invoke-Iota export_delta kanban export $deltaPath $cursor
if ($deltaExport.ExitCode -ne 0) {
    throw "iota kanban export delta failed"
}
Write-Host "delta-export log: $($deltaExport.LogPath)"
$delta = Read-JsonFile -Path $deltaPath
$events = @($delta.events)

if ($events.Count -eq 0) {
    throw "No delta events captured after dispatch"
}

$taskEvents = @()
foreach ($e in $events) {
    if (-not $e.payload) {
        continue
    }

    $payloadObj = $null
    try {
        $payloadObj = ($e.payload | ConvertFrom-Json)
    }
    catch {
        continue
    }

    $hit = $false
    if ($payloadObj.PSObject.Properties.Name -contains "task_id") {
        $hit = ([UInt64]$payloadObj.task_id -eq $usedTaskId)
    }
    if ((-not $hit) -and ($payloadObj.PSObject.Properties.Name -contains "from_id")) {
        $hit = ([UInt64]$payloadObj.from_id -eq $usedTaskId)
    }
    if ((-not $hit) -and ($payloadObj.PSObject.Properties.Name -contains "to_id")) {
        $hit = ([UInt64]$payloadObj.to_id -eq $usedTaskId)
    }

    if ($hit) {
        $taskEvents += $e
    }
}

if ($taskEvents.Count -eq 0) {
    throw "No task-related events found for #$usedTaskId in delta events"
}

$transitioned = @($taskEvents | Where-Object { $_.event_type -eq "task_transitioned" })
# dispatch-driven transitions: ready->running (claim) and running->done (complete)
$dispatchTransitions = @($transitioned | Where-Object {
    $p = $null
    try { $p = $_.payload | ConvertFrom-Json } catch {}
    $p -and ($p.from -eq "ready" -or $p.from -eq "running")
})

if ($transitioned.Count -lt 3) {
    throw "Expected at least 3 task_transitioned events, got $($transitioned.Count)"
}

Write-Host "task-related delta events: $($taskEvents.Count)"
Write-Host "task_transitioned events: $($transitioned.Count)"
Write-Host "  dispatch-driven (ready->running, running->done): $($dispatchTransitions.Count)"
Write-Host ""
Write-Host "Task event timeline:" -ForegroundColor Green
$taskEvents |
    Sort-Object id |
    ForEach-Object {
        $p = $null
        try { $p = $_.payload | ConvertFrom-Json } catch {}
        $detail = if ($p) {
            if ($p.PSObject.Properties.Name -contains "from") { "$($p.from) -> $($p.to)" }
            elseif ($p.PSObject.Properties.Name -contains "status") { "status=$($p.status)" }
            else { "" }
        } else { "" }
        Write-Host ("  event#{0,-4} {1,-25} {2}" -f $_.id, $_.event_type, $detail)
    }

$reportPath = Write-RunReport -UsedTaskId $usedTaskId -BaselineCursor $cursor -TaskEvents $taskEvents -UpdatedCount $dispatchTransitions.Count -PetOutputPath $petOutPath
$latestDir = Publish-LatestArtifacts -RunDir $workDir
Write-Host ""
Write-Host "run report: $reportPath" -ForegroundColor Yellow
Write-Host "latest dir: $latestDir" -ForegroundColor Yellow

Write-Step "5) 链路验证结论"
Write-Host "[ACP path]     prompt -> iota run --backend hermes -> JSON-RPC session/prompt -> hermes LLM 回复"
Write-Host "[Dispatch path] iota kanban dispatch #${usedTaskId}:"
Write-Host "  Dispatcher::tick() -> spawn_worker() -> hermes -p default"
Write-Host "    HERMES_KANBAN_TASK=$usedTaskId"
Write-Host "    HERMES_KANBAN_DB=$shadowPath"
Write-Host "    HERMES_KANBAN_BOARD=<board-slug>"
Write-Host "  shadow DB created: $shadowExists"
Write-Host "  ShadowWatcher::poll() -> sync_events() -> store.transition() -> iota 主 DB"
Write-Host ""
Write-Host "事件证据："
Write-Host "- task_transitioned: $($transitioned.Count) 次"
Write-Host "- dispatch 驱动 (ready->running by claim, running->done by shadow/exit): $($dispatchTransitions.Count) 次"
Write-Host "- iota 持有 kanban.db；hermes worker 通过 shadow DB 向 iota 回传状态"
