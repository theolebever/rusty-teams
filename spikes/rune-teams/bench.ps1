param([string]$Exe, [string]$Log)
$p = Start-Process -FilePath $Exe -ArgumentList "--bench" -RedirectStandardError $Log -PassThru
$cores = [Environment]::ProcessorCount
$t0 = Get-Date; $c0 = 0; $peakWs = 0; $peakPriv = 0
while (-not $p.HasExited) {
  Start-Sleep -Seconds 1; $p.Refresh()
  if ($p.HasExited) { break }
  $now = Get-Date; $cpu = $p.TotalProcessorTime.TotalSeconds; $wall = ($now - $t0).TotalSeconds
  $peakWs = [math]::Max($peakWs, $p.WorkingSet64); $peakPriv = [math]::Max($peakPriv, $p.PrivateMemorySize64)
  "t={0,5:N0}s cpu={1,5:N1}% of {2} cores, ws={3:N1} MB, private={4:N1} MB" -f $wall, (100 * ($cpu - $c0) / $wall / $cores), $cores, ($p.WorkingSet64 / 1MB), ($p.PrivateMemorySize64 / 1MB)
  $t0 = $now; $c0 = $cpu
}
"peak ws {0:N1} MB, peak private {1:N1} MB" -f ($peakWs / 1MB), ($peakPriv / 1MB)
Get-Content $Log
