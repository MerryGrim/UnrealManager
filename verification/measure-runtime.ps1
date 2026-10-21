[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [ValidateRange(1, 1800)]
    [int]$Seconds = 20,
    [string]$OutputDirectory,
    # Optional externally observed workload label. Never drives the app.
    [string]$PhaseFile
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'RuntimeEvidence.ps1')

function Get-ProcessIdentity($ProcessInfo) {
    return ([DateTime]$ProcessInfo.CreationDate).ToUniversalTime().Ticks.ToString()
}

function Assert-NoExistingManager {
    $names = @('unreal-manager.exe', 'UnrealManager.exe', 'Unreal Manager.exe', [IO.Path]::GetFileName($script:SourceExecutable))
    $existing = @(Get-CimInstance Win32_Process | Where-Object { $names -icontains $_.Name })
    if ($existing.Count -gt 0) {
        throw 'An Unreal Manager process is already running. Close it before measurement. No existing process was changed.'
    }
}

function Get-OwnedTree {
    $all = @(Get-CimInstance Win32_Process)
    $selected = @{}
    foreach ($processInfo in $all) {
        $key = [string]$processInfo.ProcessId
        if ($script:OwnedIdentities.ContainsKey($key) -and
            $script:OwnedIdentities[$key] -eq (Get-ProcessIdentity $processInfo)) {
            $selected[$key] = $processInfo
        }
    }
    do {
        $changed = $false
        foreach ($processInfo in $all) {
            $key = [string]$processInfo.ProcessId
            $parentKey = [string]$processInfo.ParentProcessId
            if (-not $selected.ContainsKey($key) -and $selected.ContainsKey($parentKey) -and
                ([DateTime]$processInfo.CreationDate) -ge ([DateTime]$selected[$parentKey].CreationDate)) {
                $selected[$key] = $processInfo
                $script:OwnedIdentities[$key] = Get-ProcessIdentity $processInfo
                $changed = $true
            }
        }
    } while ($changed)
    return @($selected.Values | Sort-Object CreationDate, ProcessId)
}

function Get-OwnedCounter($ClassName, $TreeIds) {
    try {
        $rows = @(Get-CimInstance -ClassName $ClassName | Where-Object {
            $_.Name -match '^pid_(\d+)_' -and $TreeIds -contains [int]$Matches[1]
        })
        return [PSCustomObject]@{ Available = $true; Error = $null; Rows = $rows }
    } catch {
        return [PSCustomObject]@{ Available = $false; Error = $_.Exception.Message; Rows = @() }
    }
}

function Get-LiveOwnedProcess([int]$ProcessId) {
    # Retain the Process returned by Start-Process. Its native handle/start time
    # and image validate the root even if the very first CIM attach failed.
    if ($null -ne $script:LaunchedProcess -and $ProcessId -eq $script:LaunchedProcess.Id) {
        return Get-UmVerifiedLaunchedProcess $script:LaunchedProcess $script:LaunchedNativeIdentity
    }
    $key = [string]$ProcessId
    if (-not $script:OwnedIdentities.ContainsKey($key)) { return $null }
    $live = Get-CimInstance Win32_Process -Filter "ProcessId = $ProcessId"
    if ($null -eq $live -or (Get-ProcessIdentity $live) -ne $script:OwnedIdentities[$key]) { return $null }
    $processObject = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($null -eq $processObject) { return $null }
    # CIM creation timestamps have microsecond resolution. Compare the native
    # start time too, to avoid acting on a process after a PID was reused.
    $difference = [Math]::Abs($processObject.StartTime.ToUniversalTime().Ticks - [long]$script:OwnedIdentities[$key])
    if ($difference -gt 10000) { return $null }
    return $processObject
}

function Measure-OrNull($Rows, [string]$Property, [string]$Mode) {
    if (@($Rows).Count -eq 0) { return $null }
    if ($Mode -eq 'Average') { return ($Rows | Measure-Object -Property $Property -Average).Average }
    return ($Rows | Measure-Object -Property $Property -Maximum).Maximum
}

function Get-ObservedPhase {
    if ([string]::IsNullOrWhiteSpace($PhaseFile)) { return 'idle' }
    try {
        $value = [IO.File]::ReadAllText($PhaseFile).Trim()
        if ($value -in @('idle','tray','login-open','transfer','unclassified')) { return $value }
    } catch {}
    return 'unclassified'
}

function Get-NativeExitStatus {
    $status = [ordered]@{ HasExited = $null; ExitCode = $null; ExitTime = $null; Error = $null }
    if ($null -eq $script:LaunchedProcess) { return [PSCustomObject]$status }
    try {
        $script:LaunchedProcess.Refresh()
        $status.HasExited = $script:LaunchedProcess.HasExited
        if ($status.HasExited) {
            $status.ExitCode = $script:LaunchedProcess.ExitCode
            $status.ExitTime = $script:LaunchedProcess.ExitTime.ToUniversalTime().ToString('o')
        }
    } catch { $status.Error = $_.Exception.Message }
    return [PSCustomObject]$status
}

function Close-OwnedTree {
    $cleanup = [ordered]@{ GracefulCloseRequested = $false; CloseRequestCount = 0; MainExitedGracefully = $false; NativeRootFallbackUsed = $false; ForcedProcessIds = @(); RemainingProcessIds = @(); Errors = @() }
    if ($null -eq $script:LaunchedProcess) { return [PSCustomObject]$cleanup }
    $cleanup.NativeRootFallbackUsed = -not $script:OwnedIdentities.ContainsKey([string]$script:LaunchedProcess.Id)
    try {
        # Process.MainWindowHandle may identify a secondary Epic window. Once
        # it closes, resolve the same verified process again before requesting
        # the application's main window to close. No arbitrary window handles.
        $closeDeadline = [DateTime]::UtcNow.AddSeconds(15)
        do {
            $main = Get-LiveOwnedProcess $script:LaunchedProcess.Id
            if ($null -eq $main) { $cleanup.MainExitedGracefully = $true; break }
            if ($main.CloseMainWindow()) {
                $cleanup.GracefulCloseRequested = $true
                $cleanup.CloseRequestCount++
            }
            if ($main.WaitForExit(1000)) { $cleanup.MainExitedGracefully = $true; break }
        } while ([DateTime]::UtcNow -lt $closeDeadline)
        # Stop only the launched main process if graceful shutdown timed out.
        $main = Get-LiveOwnedProcess $script:LaunchedProcess.Id
        if ($null -ne $main) {
            Stop-Process -InputObject $main -Force
            $cleanup.ForcedProcessIds += $main.Id
        }
        # Let WebView2 terminate normally after its owning main process exits.
        $deadline = [DateTime]::UtcNow.AddSeconds(5)
        do {
            $remaining = @(Get-OwnedTree)
            if ($remaining.Count -eq 0) { break }
            Start-Sleep -Milliseconds 250
        } while ([DateTime]::UtcNow -lt $deadline)
        foreach ($processInfo in $remaining) {
            try {
                $owned = Get-LiveOwnedProcess ([int]$processInfo.ProcessId)
                if ($null -ne $owned) {
                    Stop-Process -InputObject $owned -Force
                    $cleanup.ForcedProcessIds += $owned.Id
                }
            } catch { $cleanup.Errors += $_.Exception.Message }
        }
        $cleanup.RemainingProcessIds = @((Get-OwnedTree) | ForEach-Object { [int]$_.ProcessId })
    } catch { $cleanup.Errors += $_.Exception.Message }
    return [PSCustomObject]$cleanup
}

$script:SourceExecutable = (Resolve-Path -LiteralPath $Executable).ProviderPath
if (-not (Test-Path -LiteralPath $script:SourceExecutable -PathType Leaf) -or [IO.Path]::GetExtension($script:SourceExecutable) -ine '.exe') {
    throw 'Executable must name an existing Windows .exe file.'
}
Assert-NoExistingManager
$base = if ([string]::IsNullOrWhiteSpace($OutputDirectory)) { Join-Path $PSScriptRoot 'current' } else { [IO.Path]::GetFullPath($OutputDirectory) }
$stamp = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH-mm-ss-fffZ')
$runDirectory = Join-Path $base ("runtime-$stamp-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
$isolatedDirectory = Join-Path $runDirectory 'isolated'
$settingsDirectory = Join-Path $isolatedDirectory 'Data\Settings'
[void](New-Item -ItemType Directory -Path $settingsDirectory -Force)
$copiedExecutable = Join-Path $isolatedDirectory ([IO.Path]::GetFileName($script:SourceExecutable))
Copy-Item -LiteralPath $script:SourceExecutable -Destination $copiedExecutable
[IO.File]::WriteAllText((Join-Path $isolatedDirectory 'portable.marker'), '', [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText((Join-Path $settingsDirectory 'window.json'), '"exit"', [Text.UTF8Encoding]::new($false))

$script:OwnedIdentities = @{}
$script:LaunchedProcess = $null
$script:LaunchedNativeIdentity = $null
$previousCpu = @{}
$samples = [Collections.Generic.List[object]]::new()
$observedProcesses = @{}
$logicalProcessors = [Environment]::ProcessorCount
$warmupSeconds = 5
$measurementError = $null
$cleanup = $null
$startedAt = [DateTime]::UtcNow
$samplingStartedAt = $null
$samplingCompleted = $false
$exitBeforeCleanup = $null
$exitAfterCleanup = $null
$stdoutPath = Join-Path $runDirectory 'native-stdout.log'
$stderrPath = Join-Path $runDirectory 'native-stderr.log'

try {
    Assert-NoExistingManager
    $script:LaunchedProcess = Start-Process -FilePath $copiedExecutable -WorkingDirectory $isolatedDirectory -WindowStyle Hidden -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -PassThru
    $script:LaunchedNativeIdentity = Get-UmLaunchedProcessIdentity $script:LaunchedProcess $copiedExecutable
    if ($null -eq $script:LaunchedNativeIdentity) { throw 'The isolated executable exited before native identity attachment.' }
    $rootInfo = Get-CimInstance Win32_Process -Filter ("ProcessId = " + $script:LaunchedProcess.Id)
    if ($null -eq $rootInfo) { throw 'The isolated executable exited before measurement could attach.' }
    $rootStartDifference = [Math]::Abs($script:LaunchedProcess.StartTime.ToUniversalTime().Ticks - [long](Get-ProcessIdentity $rootInfo))
    if ($rootStartDifference -gt 10000) { throw 'The launched process identity changed before measurement could attach.' }
    $script:OwnedIdentities[[string]$rootInfo.ProcessId] = Get-ProcessIdentity $rootInfo
    if ($rootInfo.ExecutablePath -ine $copiedExecutable) { throw 'The launched executable path could not be validated.' }
    Start-Sleep -Seconds $warmupSeconds
    $samplingStartedAt = [DateTime]::UtcNow
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $previousElapsed = $null
    do {
        $tree = @(Get-OwnedTree)
        $treeIds = @($tree | ForEach-Object { [int]$_.ProcessId })
        if ($treeIds -notcontains $script:LaunchedProcess.Id) { throw 'The isolated main process exited during the sample interval.' }
        $elapsed = $timer.Elapsed.TotalSeconds
        $interval = if ($null -eq $previousElapsed) { $null } else { $elapsed - $previousElapsed }
        $cpuDelta = 0.0
        $workingSet = 0.0
        $privateBytes = 0.0
        $processRows = @()
        foreach ($processInfo in $tree) {
            $key = ([string]$processInfo.ProcessId) + ':' + (Get-ProcessIdentity $processInfo)
            $cpu = ([double]$processInfo.KernelModeTime + [double]$processInfo.UserModeTime) / 10000000.0
            $delta = if ($previousCpu.ContainsKey($key)) { [Math]::Max(0.0, $cpu - $previousCpu[$key]) } else { $null }
            if ($null -ne $delta) { $cpuDelta += $delta }
            $previousCpu[$key] = $cpu
            $workingSet += [double]$processInfo.WorkingSetSize
            $privateBytes += [double]$processInfo.PrivatePageCount
            # This smoke measures the main window. Its browser is a direct
            # child of the native app. Exiting descendants may have a null CIM
            # command line; null must not be interpreted as "no --type switch".
            $isWebViewBrowser = $processInfo.Name -ieq 'msedgewebview2.exe' -and
                $processInfo.ParentProcessId -eq $script:LaunchedProcess.Id -and
                -not [string]::IsNullOrWhiteSpace($processInfo.CommandLine) -and
                $processInfo.CommandLine -notmatch '(?:^|\s)--type='
            $row = [PSCustomObject]@{
                ProcessId = [int]$processInfo.ProcessId; ParentProcessId = [int]$processInfo.ParentProcessId
                Name = $processInfo.Name; CreatedAt = ([DateTime]$processInfo.CreationDate).ToUniversalTime().ToString('o')
                ExecutablePath = $processInfo.ExecutablePath; CommandLine = $processInfo.CommandLine
                CpuSeconds = $cpu; CpuDeltaSeconds = $delta
                WorkingSetBytes = [double]$processInfo.WorkingSetSize; PrivateBytes = [double]$processInfo.PrivatePageCount
                IsRootWebViewBrowser = $isWebViewBrowser
                DisableGpuFlag = $isWebViewBrowser -and $processInfo.CommandLine -match '(?:^|\s|")--disable-gpu(?:\s|$|")'
                DisableGpuCompositingFlag = $isWebViewBrowser -and $processInfo.CommandLine -match '(?:^|\s|")--disable-gpu-compositing(?:\s|$|")'
            }
            $processRows += $row
            $observedProcesses[$key] = $row
        }
        $engine = Get-OwnedCounter 'Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine' $treeIds
        $gpuMemory = Get-OwnedCounter 'Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory' $treeIds
        $engineRows = @($engine.Rows | ForEach-Object {
            [PSCustomObject]@{ Name = $_.Name; UtilizationPercent = [double]$_.UtilizationPercentage; RunningTime = [double]$_.RunningTime }
        })
        $memoryRows = @($gpuMemory.Rows | ForEach-Object {
            [PSCustomObject]@{ Name = $_.Name; DedicatedBytes = [double]$_.DedicatedUsage; SharedBytes = [double]$_.SharedUsage; TotalCommittedBytes = [double]$_.TotalCommitted }
        })
        $samples.Add([PSCustomObject]@{
            Timestamp = [DateTime]::UtcNow.ToString('o'); ElapsedSeconds = $elapsed; IntervalSeconds = $interval
            CpuDeltaSeconds = if ($null -eq $interval) { $null } else { $cpuDelta }
            CpuPercentOneCore = if ($null -ne $interval -and $interval -gt 0) { 100.0 * $cpuDelta / $interval } else { $null }
            CpuPercentAllLogicalProcessors = if ($null -ne $interval -and $interval -gt 0) { 100.0 * $cpuDelta / $interval / $logicalProcessors } else { $null }
            WorkingSetBytes = $workingSet; PrivateBytes = $privateBytes; Processes = $processRows
            ObservedPhase = Get-ObservedPhase
            GpuEngineCounterAvailable = $engine.Available; GpuEngineCounterError = $engine.Error; GpuEngines = $engineRows
            GpuMemoryCounterAvailable = $gpuMemory.Available; GpuMemoryCounterError = $gpuMemory.Error; GpuMemory = $memoryRows
        })
        $previousElapsed = $elapsed
        if ($timer.Elapsed.TotalSeconds -ge $Seconds) { break }
        $sleepMs = [Math]::Min(1000, [Math]::Max(1, [int](($Seconds - $timer.Elapsed.TotalSeconds) * 1000)))
        Start-Sleep -Milliseconds $sleepMs
    } while ($true)
    $samplingCompleted = $true
} catch {
    $measurementError = $_.Exception.Message
} finally {
    $exitBeforeCleanup = Get-NativeExitStatus
    $cleanup = Close-OwnedTree
    $exitAfterCleanup = Get-NativeExitStatus
}

$softwareEvidence = if ($null -ne $script:LaunchedProcess) { Get-UmSoftwareRenderingEvidence -Samples @($samples.ToArray()) -RootProcessId $script:LaunchedProcess.Id } else { Get-UmSoftwareRenderingEvidence -Samples @() -RootProcessId 0 }
$rootBrowsers = @($samples | ForEach-Object { $_.Processes } | Where-Object { $_.IsRootWebViewBrowser } | Group-Object ProcessId,CreatedAt | ForEach-Object { $_.Group[-1] })
$unreadableRootBrowsers = @($observedProcesses.Values | Where-Object {
    $_.Name -ieq 'msedgewebview2.exe' -and $_.ParentProcessId -eq $script:LaunchedProcess.Id -and
        [string]::IsNullOrWhiteSpace($_.CommandLine)
})
$unreadableWebViewDescendants = @($observedProcesses.Values | Where-Object {
    $_.Name -ieq 'msedgewebview2.exe' -and $_.ParentProcessId -ne $script:LaunchedProcess.Id -and
        [string]::IsNullOrWhiteSpace($_.CommandLine)
})
$gpuRows = @($samples | ForEach-Object { $_.GpuEngines })
$memoryRows = @($samples | ForEach-Object { $_.GpuMemory })
$cpuSamples = @($samples | Where-Object { $null -ne $_.CpuPercentOneCore })
$phaseSummaries = @(foreach ($group in ($samples | Group-Object ObservedPhase)) {
    $rows = @($group.Group)
    $withCpu = @($rows | Where-Object { $null -ne $_.CpuPercentOneCore })
    [pscustomobject]@{
        Phase = $group.Name; SampleCount = $rows.Count
        FirstTimestamp = $rows[0].Timestamp; LastTimestamp = $rows[-1].Timestamp
        CpuPercentOneCoreAverage = Measure-OrNull $withCpu 'CpuPercentOneCore' 'Average'
        CpuPercentOneCorePeak = Measure-OrNull $withCpu 'CpuPercentOneCore' 'Maximum'
        PrivateBytesFirst = $rows[0].PrivateBytes; PrivateBytesLast = $rows[-1].PrivateBytes
        PrivateBytesDelta = $rows[-1].PrivateBytes - $rows[0].PrivateBytes
        WorkingSetBytesPeak = Measure-OrNull $rows 'WorkingSetBytes' 'Maximum'
    }
})
$report = [ordered]@{
    StartedAt = $startedAt.ToString('o'); FinishedAt = [DateTime]::UtcNow.ToString('o')
    SourceExecutable = $script:SourceExecutable; SourceSha256 = (Get-FileHash -LiteralPath $script:SourceExecutable -Algorithm SHA256).Hash
    IsolatedExecutable = $copiedExecutable; OutputDirectory = $runDirectory
    PortableDataDirectory = (Join-Path $isolatedDirectory 'Data'); CloseBehavior = 'exit'
    RequestedSampleSeconds = $Seconds; WarmupSeconds = $warmupSeconds; SamplingStartedAt = if ($null -ne $samplingStartedAt) { $samplingStartedAt.ToString('o') } else { $null }
    SampleCount = $samples.Count; SamplingCompleted = $samplingCompleted; MeasurementError = $measurementError
    LaunchWindowStyle = 'Hidden'; NativeStdout = $stdoutPath; NativeStderr = $stderrPath
    NativeExitBeforeCleanup = $exitBeforeCleanup; NativeExitAfterCleanup = $exitAfterCleanup
    LogicalProcessors = $logicalProcessors; PowerShellVersion = $PSVersionTable.PSVersion.ToString()
    SoftwareFlagsObserved = $softwareEvidence.SoftwareFlagsObserved
    SoftwareRenderingEvidence = $softwareEvidence
    LaunchedNativeIdentity = $script:LaunchedNativeIdentity
    UnreadableRootBrowserCandidates = $unreadableRootBrowsers
    UnreadableWebViewDescendants = $unreadableWebViewDescendants
    RootBrowserScope = 'Full sample history: known command line, no --type switch, direct native-app child. Every readable root observation needs both flags. Unreadable candidates remain raw unknowns; they prevent a flag pass unless the exact PID+creation identity also has a readable root observation. Unknown/reused identities are not covered.'
    RootWebViewBrowsers = $rootBrowsers; ObservedProcesses = @($observedProcesses.Values | Sort-Object CreatedAt, ProcessId)
    CpuPercentOneCoreAverage = Measure-OrNull $cpuSamples 'CpuPercentOneCore' 'Average'
    CpuPercentOneCorePeak = Measure-OrNull $cpuSamples 'CpuPercentOneCore' 'Maximum'
    CpuPercentAllLogicalProcessorsAverage = Measure-OrNull $cpuSamples 'CpuPercentAllLogicalProcessors' 'Average'
    WorkingSetBytesAverage = Measure-OrNull $samples 'WorkingSetBytes' 'Average'
    WorkingSetBytesPeak = Measure-OrNull $samples 'WorkingSetBytes' 'Maximum'
    PrivateBytesPeak = Measure-OrNull $samples 'PrivateBytes' 'Maximum'
    GpuEngineProviderAvailableEverySample = $samples.Count -gt 0 -and @($samples | Where-Object { -not $_.GpuEngineCounterAvailable }).Count -eq 0
    GpuMemoryProviderAvailableEverySample = $samples.Count -gt 0 -and @($samples | Where-Object { -not $_.GpuMemoryCounterAvailable }).Count -eq 0
    GpuOwnedEngineRowCount = $gpuRows.Count
    GpuActivityObserved = if ($gpuRows.Count -gt 0) { @($gpuRows | Where-Object { $_.UtilizationPercent -gt 0 }).Count -gt 0 } else { $null }
    GpuSingleEngineUtilizationPercentPeak = Measure-OrNull $gpuRows 'UtilizationPercent' 'Maximum'
    GpuDedicatedBytesSingleInstancePeak = Measure-OrNull $memoryRows 'DedicatedBytes' 'Maximum'
    PhaseFileUsed = -not [string]::IsNullOrWhiteSpace($PhaseFile)
    PhaseSummaries = $phaseSummaries
    Cleanup = $cleanup; Samples = @($samples.ToArray())
    Interpretation = @(
        'Fresh separate portable profile with no copied Epic account. Without a phase file this is an idle smoke. External phase labels document separately observed interactions.',
        'Software flags describe WebView2 configuration. GPU counters describe only activity observed for owned process IDs during this sample interval.',
        'Missing providers or missing owned PID counter rows are unavailable telemetry, not proof of zero GPU use. Formatted utilization can round small activity to zero.',
        'A WebView2 gpu-process may exist even with software flags; its name alone does not prove hardware acceleration or GPU power state.',
        'CPU deltas cover identities alive in consecutive samples; short-lived processes between samples can be missed. CPU percent one core can exceed 100; machine percent divides by logical processor count.',
        'Summed working sets can count shared pages more than once. Hidden-start measurement does not establish tray, login, transfer or interactive workload performance.'
        'Optional phase labels are external observations, not an automated workload or evidence of successful authentication/transfer. Memory deltas are descriptive and do not prove absence of leaks.'
    )
}
$reportPath = Join-Path $runDirectory 'runtime-report.json'
[IO.File]::WriteAllText($reportPath, ($report | ConvertTo-Json -Depth 12), [Text.UTF8Encoding]::new($false))
Write-Output "Runtime report: $reportPath"
Write-Output ("Samples: {0}; software flags observed: {1}; owned GPU counter rows: {2}; GPU activity observed: {3}" -f $samples.Count, $report.SoftwareFlagsObserved, $gpuRows.Count, $report.GpuActivityObserved)
if ($null -ne $measurementError) { throw "Runtime measurement failed: $measurementError. See $reportPath" }
if ($cleanup.RemainingProcessIds.Count -gt 0 -or $cleanup.Errors.Count -gt 0) { throw "Owned-process cleanup was incomplete. See $reportPath" }
if (-not $report.SoftwareFlagsObserved) { throw "Expected WebView2 software-rendering flags were not observed. See $reportPath" }
