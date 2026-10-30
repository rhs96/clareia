$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$warnings = @()
$tcpAvailable = $true; $udpAvailable = $true; $netAvailable = $true; $gpuAvailable = $true
$connections = @()
try {
 $connections = @(Get-NetTCPConnection -State Established -ErrorAction Stop | ForEach-Object {
  @{ pid = $_.OwningProcess; protocol='TCP'; remote = $_.RemoteAddress; port = $_.RemotePort; local = $_.LocalAddress; localPort = $_.LocalPort }
 })
} catch { $tcpAvailable = $false; $warnings += 'Não foi possível consultar conexões TCP.' }
$udp = @()
try {
 $udp = @(Get-NetUDPEndpoint -ErrorAction Stop | ForEach-Object {
  @{ pid=$_.OwningProcess; protocol='UDP'; local=$_.LocalAddress; localPort=$_.LocalPort }
 })
} catch { $udpAvailable = $false; $warnings += 'Não foi possível consultar sockets UDP.' }
$adapters = @()
try {
 $adapters = @(Get-NetAdapter -ErrorAction Stop | Where-Object Status -eq 'Up' | Get-NetAdapterStatistics | ForEach-Object {
  @{ name = $_.Name; received = [double]$_.ReceivedBytes; sent = [double]$_.SentBytes }
 })
} catch { $netAvailable = $false; $warnings += 'Não foi possível consultar os contadores de rede.' }
$gpu = @{}
try {
 $engines = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -ErrorAction Stop)
 if ($engines.Count -eq 0) { $gpuAvailable = $false; $warnings += 'O driver não forneceu contadores de GPU.' }
 foreach ($engine in $engines) {
  if ($engine.Name -match '^pid_(\d+)_') {
   $key = $matches[1]; $value = [Math]::Min(100,[Math]::Max(0,[double]$engine.UtilizationPercentage))
   if (-not $gpu.ContainsKey($key)) { $gpu[$key] = @{percent=0;engine=''} }
   if ($value -ge $gpu[$key].percent) { $gpu[$key] = @{percent=$value;engine=$engine.Name} }
  }
 }
} catch { $gpuAvailable=$false; $warnings += 'Não foi possível consultar a GPU. Verifique o driver e os contadores do Windows.' }
$processes = @(Get-Process | ForEach-Object {
 $p = $_; $pathValue = ''; $started = ''; $ticks = ''
 try { $started=$p.StartTime.ToUniversalTime().ToString('o'); $ticks=$p.StartTime.ToUniversalTime().ToFileTimeUtc().ToString() } catch {}
 try { $pathValue=$p.Path } catch {}
 $gpuValue=$null; $gpuEngine=''
 if ($gpuAvailable) { $gpuValue=0; if ($gpu.ContainsKey([string]$p.Id)) { $gpuValue=$gpu[[string]$p.Id].percent; $gpuEngine=$gpu[[string]$p.Id].engine } }
 @{ pid = $p.Id; name = $p.ProcessName; path = $pathValue; started = $started; startedTicks=$ticks; cpu = [double]$p.CPU; memory = [double]$p.WorkingSet64; gpu=$gpuValue; gpuEngine=$gpuEngine }
})
@{ tcpAvailable=$tcpAvailable; udpAvailable=$udpAvailable; gpuAvailable=$gpuAvailable; netAvailable=$netAvailable; time=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds(); cores=[Environment]::ProcessorCount; adapters=$adapters; processes=$processes; connections=$connections; udp=$udp; warnings=$warnings } | ConvertTo-Json -Depth 6 -Compress
