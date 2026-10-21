# Read-only helpers. No app, counter collection, process stop or file write.
function Get-UmCreationTicks($Value) {
    if ($Value -is [DateTimeOffset]) { return $Value.UtcDateTime.Ticks }
    if ($Value -is [DateTime]) {
        if ($Value.Kind -eq [DateTimeKind]::Unspecified) { return $null }
        return $Value.ToUniversalTime().Ticks
    }
    if ($Value -isnot [string] -or $Value -notmatch '^\d{4}-\d{2}-\d{2}T.+(?:Z|[+-]\d{2}:\d{2})$') { return $null }
    try { return [DateTimeOffset]::Parse($Value,[Globalization.CultureInfo]::InvariantCulture).UtcDateTime.Ticks } catch { return $null }
}

function Get-UmSoftwareRenderingEvidence {
    param([object[]]$Samples,[int]$RootProcessId)
    $readable = [Collections.Generic.List[object]]::new()
    $unknown = [Collections.Generic.List[object]]::new()
    $known = @{}
    foreach ($sample in $Samples) { foreach ($row in $sample.Processes) {
        if ($row.Name -ine 'msedgewebview2.exe' -or [int]$row.ParentProcessId -ne $RootProcessId) { continue }
        $ticks = Get-UmCreationTicks $row.CreatedAt
        $identity = if ($null -ne $ticks) { '{0}:{1}' -f $row.ProcessId,$ticks } else { $null }
        $observation = [pscustomobject]@{
            Timestamp=$sample.Timestamp;ProcessId=[int]$row.ProcessId;ParentProcessId=[int]$row.ParentProcessId
            CreatedAt=$row.CreatedAt;Identity=$identity;Readable=-not [string]::IsNullOrWhiteSpace($row.CommandLine)
            DisableGpuFlag=$false;DisableGpuCompositingFlag=$false;CoveredByReadableRootIdentity=$false
        }
        if (-not $observation.Readable) { $unknown.Add($observation); continue }
        if ($row.CommandLine -match '(?:^|\s|")--type=') { continue }
        $observation.DisableGpuFlag = $row.CommandLine -cmatch '(?:^|\s|")--disable-gpu(?:\s|$|")'
        $observation.DisableGpuCompositingFlag = $row.CommandLine -cmatch '(?:^|\s|")--disable-gpu-compositing(?:\s|$|")'
        $readable.Add($observation)
        if ($null -ne $identity) { $known[$identity]=$true }
    }}
    foreach ($row in $unknown) { $row.CoveredByReadableRootIdentity = $null -ne $row.Identity -and $known.ContainsKey($row.Identity) }
    $invalid = @($readable | Where-Object { $null -eq $_.Identity -or -not ($_.DisableGpuFlag -and $_.DisableGpuCompositingFlag) })
    $uncovered = @($unknown | Where-Object { -not $_.CoveredByReadableRootIdentity })
    [pscustomobject]@{
        Schema=1;SoftwareFlagsObserved=$readable.Count -gt 0 -and $invalid.Count -eq 0 -and $uncovered.Count -eq 0
        RootProcessId=$RootProcessId;RootBrowserIdentityCount=$known.Count
        ReadableRootObservations=@($readable.ToArray());InvalidRootObservations=$invalid
        UnreadableRootObservations=@($unknown.ToArray());UncoveredUnreadableRootObservations=$uncovered
        CoveredUnreadableRootObservationCount=$unknown.Count-$uncovered.Count
        Scope='Full sample history grouped by PID plus exact UTC creation time. All readable root observations need both flags. Null observations remain unknown; only an observed same-identity root covers them. PID reuse never covers an unknown identity. Configuration evidence only, not measured GPU activity.'
    }
}

function Get-UmLaunchedProcessIdentity {
    param([Diagnostics.Process]$Process,[string]$ExpectedExecutable)
    $Process.Refresh()
    if ($Process.HasExited) { return $null }
    $image=$Process.Path
    if ([string]::IsNullOrWhiteSpace($image) -or -not [string]::Equals([IO.Path]::GetFullPath($image),[IO.Path]::GetFullPath($ExpectedExecutable),[StringComparison]::OrdinalIgnoreCase)) { throw 'Returned launch process image does not match the owned executable.' }
    [pscustomobject]@{ProcessId=$Process.Id;StartTicks=$Process.StartTime.ToUniversalTime().Ticks;ExecutablePath=[IO.Path]::GetFullPath($image)}
}

function Get-UmVerifiedLaunchedProcess {
    param([Diagnostics.Process]$Process,$Identity)
    if ($null -eq $Process) { return $null }
    $Process.Refresh()
    if ($Process.HasExited) { return $null }
    if ($null -eq $Identity -or $Process.Id -ne $Identity.ProcessId -or $Process.StartTime.ToUniversalTime().Ticks -ne $Identity.StartTicks -or [string]::IsNullOrWhiteSpace($Process.Path) -or -not [string]::Equals([IO.Path]::GetFullPath($Process.Path),$Identity.ExecutablePath,[StringComparison]::OrdinalIgnoreCase)) { throw 'Owned launch identity could not be revalidated; no process action was authorized.' }
    return $Process
}
