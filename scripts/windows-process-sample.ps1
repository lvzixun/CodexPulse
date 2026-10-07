# Read-only process-tree samples. Does not record command lines, paths or account data.
param(
  [Parameter(Mandatory)][int]$ProcessId,
  [ValidateRange(2, 3600)][int]$DurationSeconds = 60,
  [ValidateRange(1, 60)][int]$IntervalSeconds = 2,
  [Parameter(Mandatory)][string]$OutputPath
)
$ErrorActionPreference = 'Stop'
$started = [Diagnostics.Stopwatch]::StartNew()
$records = [Collections.Generic.List[object]]::new()
while ($started.Elapsed.TotalSeconds -lt $DurationSeconds) {
  $inventory = @(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,Name)
  if (!($inventory | Where-Object ProcessId -EQ $ProcessId)) { throw 'Host exited during sampling' }
  $members = [Collections.Generic.HashSet[int]]::new()
  [void]$members.Add($ProcessId)
  do {
    $changed = $false
    foreach ($entry in $inventory) {
      if ($members.Contains([int]$entry.ParentProcessId) -and $members.Add([int]$entry.ProcessId)) { $changed = $true }
    }
  } while ($changed)
  $processes = @($members | ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue })
  $records.Add([pscustomobject]@{
    elapsed_seconds = [math]::Round($started.Elapsed.TotalSeconds, 3)
    process_count = $processes.Count
    cpu_seconds = ($processes | Measure-Object CPU -Sum).Sum
    private_commit_bytes = ($processes | Measure-Object PrivateMemorySize64 -Sum).Sum
    working_set_bytes = ($processes | Measure-Object WorkingSet64 -Sum).Sum
    handles = ($processes | Measure-Object HandleCount -Sum).Sum
    quota_cli_count = @($inventory | Where-Object { $members.Contains([int]$_.ProcessId) -and $_.Name -match '^codex(\.exe)?$' }).Count
  })
  Start-Sleep -Seconds $IntervalSeconds
}
$records | ConvertTo-Json | Set-Content -LiteralPath $OutputPath -Encoding utf8
$records | Measure-Object private_commit_bytes,working_set_bytes,handles,process_count -Maximum
