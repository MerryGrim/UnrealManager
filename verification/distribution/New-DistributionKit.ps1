#requires -Version 5.1
[CmdletBinding()]
param([Parameter(Mandatory)][string]$OutputDirectory)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$out=[IO.Path]::GetFullPath($OutputDirectory)
if (-not [IO.Path]::IsPathRooted($OutputDirectory) -or $out.StartsWith('\\') -or (Test-Path -LiteralPath $out)) { throw 'Choose a fresh absolute local workspace directory.' }
$ancestor=[IO.Path]::GetDirectoryName($out)
while($ancestor){if(Test-Path -LiteralPath $ancestor){if((Get-Item -LiteralPath $ancestor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Linked output ancestor rejected.'}};$next=[IO.Path]::GetDirectoryName($ancestor);if($next -eq $ancestor){break};$ancestor=$next}
$utf8=New-Object Text.UTF8Encoding($false)
$guid=[string](Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Cryptography').MachineGuid
$sha=[Security.Cryptography.SHA256]::Create()
try{$fingerprint=([BitConverter]::ToString($sha.ComputeHash($utf8.GetBytes($guid)))).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
[void][IO.Directory]::CreateDirectory($out)
$names=@('Invoke-DistributionCase.ps1','New-DistributionKit.ps1','Audit-DistributionHost.ps1','README.md','case-matrix.json')
$hashes=foreach($name in $names){$inputFile=Join-Path $PSScriptRoot $name;if((Get-Item -LiteralPath $inputFile -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Linked kit input rejected.'};[IO.File]::Copy($inputFile,(Join-Path $out $name),$false);@{name=$name;sha256=(Get-FileHash -LiteralPath $inputFile -Algorithm SHA256).Hash.ToLowerInvariant()}}
[IO.File]::WriteAllText((Join-Path $out 'forbidden-host-context.json'),(@{schema=1;createdUtc=[DateTime]::UtcNow.ToString('o');hostFingerprint=$fingerprint;purpose='Accidental execution guard; private evidence only, never a published source file.'}|ConvertTo-Json),$utf8)
[IO.File]::WriteAllText((Join-Path $out 'kit-manifest.json'),(@{schema=1;files=$hashes;runtimeExecuted=$false;cleanVmTestsRun=$false}|ConvertTo-Json -Depth 5),$utf8)
Write-Output $out
