[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$RuntimeReportPath)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$reportPath=(Resolve-Path -LiteralPath $RuntimeReportPath).ProviderPath
$runtime=Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
if (-not ('UmDxgiLuidAudit' -as [type])) { Add-Type -Path (Join-Path $PSScriptRoot 'DxgiLuidAudit.cs') }
$adapters=@([UmDxgiLuidAudit]::Read() | Select-Object LuidKey,Description,Software,Flags)
$mapped=@(foreach ($sample in $runtime.Samples) { foreach ($row in $sample.GpuEngines) {
    $match=[regex]::Match($row.Name,'(?i)^pid_(\d+)_luid_0x([0-9a-f]{1,8})_0x([0-9a-f]{1,8})_phys_')
    $key=$null; $counterPid=$null; $adapterMatches=@(); $classification='unparsed'
    if ($match.Success) {
        $counterPid=[int]$match.Groups[1].Value
        $key='{0:X8}:{1:X8}' -f [Convert]::ToUInt32($match.Groups[2].Value,16),[Convert]::ToUInt32($match.Groups[3].Value,16)
        $adapterMatches=@($adapters | Where-Object LuidKey -eq $key)
        $classification=if ($adapterMatches.Count -eq 1) {
            if ($adapterMatches[0].Software) { 'software' } else { 'hardware' }
        } elseif ($adapterMatches.Count -eq 0) { 'unmapped' } else { 'ambiguous' }
        if (@($sample.Processes | Where-Object ProcessId -eq $counterPid).Count -ne 1) { $classification='unattributed' }
    }
    $phase=if ($sample.PSObject.Properties.Name -contains 'ObservedPhase') { $sample.ObservedPhase } else { 'idle' }
    [pscustomobject]@{
        Timestamp=$sample.Timestamp; Phase=$phase; Name=$row.Name; ProcessId=$counterPid; LuidKey=$key
        Classification=$classification; UtilizationPercent=$row.UtilizationPercent
    }
}})
$software=@($mapped | Where-Object Classification -eq 'software')
$hardware=@($mapped | Where-Object Classification -eq 'hardware')
$receipt=[ordered]@{
    schema=1; classifiedUtc=[DateTime]::UtcNow.ToString('o')
    runtimeSha256=(Get-FileHash -LiteralPath $reportPath -Algorithm SHA256).Hash.ToLowerInvariant()
    mapperSha256=(Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'DxgiLuidAudit.cs') -Algorithm SHA256).Hash.ToLowerInvariant()
    sampleCount=@($runtime.Samples).Count; samplingCompleted=$runtime.SamplingCompleted
    providerAvailableEverySample=$runtime.GpuEngineProviderAvailableEverySample
    softwareFlagsObserved=$runtime.SoftwareFlagsObserved
    softwareRows=$software.Count; hardwareRows=$hardware.Count
    unclassifiedRows=@($mapped | Where-Object Classification -notin @('software','hardware')).Count
    softwarePeak=if ($software.Count) { ($software | Measure-Object UtilizationPercent -Maximum).Maximum } else { $null }
    hardwarePeak=if ($hardware.Count) { ($hardware | Measure-Object UtilizationPercent -Maximum).Maximum } else { $null }
    adapters=$adapters; rows=$mapped
    limits='Sampled owned PIDs and interval only. No hardware rows is unavailable hardware telemetry, not measured zero. Run on the same boot immediately after measurement; adapter resets can leave unmapped LUIDs. Formatted counters round small activity; separate engines are not summed.'
}
$out=Join-Path (Split-Path -Parent $reportPath) ('gpu-classification-'+[Guid]::NewGuid().ToString('N')+'.json')
$stream=[IO.File]::Open($out,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
try { $bytes=[Text.UTF8Encoding]::new($false).GetBytes(($receipt | ConvertTo-Json -Depth 9)); $stream.Write($bytes,0,$bytes.Length) }
finally { $stream.Dispose() }
Write-Output $out
