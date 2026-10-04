# Start SolarCraft's realm, then run benilla against it at native desktop size.
# Borderless fullscreen (gxWindow 0) is 3440x1440 on this machine; Hor+ FOV keeps
# 16:9's vertical field and shows extra world on the sides.
$ErrorActionPreference = "Stop"
$benilla = $PSScriptRoot
$wowRoot = Split-Path $benilla -Parent
$solar = Join-Path $wowRoot "SolarCraft"
$client = Join-Path $solar "client"

$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if (Test-Path $cargoBin) {
    $env:Path = "$cargoBin;$env:Path"
}

$wowLink = Join-Path $benilla "WoW"
if (-not (Test-Path $wowLink)) {
    if (-not (Test-Path $client)) { throw "Missing SolarCraft client at $client" }
    New-Item -ItemType Junction -Path $wowLink -Target $client | Out-Null
    Write-Host "Linked $wowLink -> $client"
}

& (Join-Path $solar "scripts\start.ps1")

function Test-Listen([int]$Port) {
    try {
        $c = New-Object System.Net.Sockets.TcpClient
        $iar = $c.BeginConnect("127.0.0.1", $Port, $null, $null)
        $ok = $iar.AsyncWaitHandle.WaitOne(400)
        $connected = $ok -and $c.Connected
        try { $c.Close() } catch {}
        return $connected
    } catch {
        return $false
    }
}

function Wait-Listen([int]$Port, [int]$Seconds, [string]$Name, [string]$ProcessName) {
    Write-Host "Waiting for $Name on 127.0.0.1:$Port ..."
    $seen = $false
    for ($i = 0; $i -lt $Seconds; $i++) {
        if (Test-Listen $Port) {
            Write-Host "$Name ready ($i s)"
            return
        }
        if ($ProcessName) {
            $alive = [bool](Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)
            if ($alive) { $seen = $true }
            elseif ($seen -or ($i -ge 15)) {
                throw "$Name process '$ProcessName' exited before opening port $Port. See $solar\server\run\logs\Server.log"
            }
        }
        Start-Sleep -Seconds 1
    }
    throw "$Name did not listen on $Port within ${Seconds}s. See $solar\server\run\logs\Server.log"
}

Wait-Listen 3724 30 "realmd" "realmd"
# mangosd binds 8085 only after maps/bots load (~30s). Handshake before that is WSAECONNREFUSED.
Wait-Listen 8085 180 "world" "mangosd"

$env:WOW_DATA = Join-Path $client "Data"
$env:WOW_HOST = "127.0.0.1"
# Hor+ is the default. WOW_FOV=reference uses 1.12's shrinking vertical field.

# Hitch telemetry: streamer CSV, 1 Hz FPS/GPU journal, live shader compiles, 5 s
# frame-delta watch. Farclip and WorldDetail stay as in config.toml. Override any
# of these by setting the env var before Play Benilla.bat.
$diag = Join-Path $benilla "benilla-config\Diagnostics"
New-Item -ItemType Directory -Force -Path $diag | Out-Null
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
if (-not $env:WOW_STREAM_TRACE) { $env:WOW_STREAM_TRACE = Join-Path $diag "stream-$stamp.csv" }
if (-not $env:WOW_FPS_JOURNAL) { $env:WOW_FPS_JOURNAL = Join-Path $diag "fps-$stamp.csv" }
if (-not $env:WOW_PIPE_TRACE) { $env:WOW_PIPE_TRACE = Join-Path $diag "pipe-$stamp.txt" }
if (-not $env:WOW_STALL) { $env:WOW_STALL = "0" }
if (-not $env:WOW_PERF_HUD) { $env:WOW_PERF_HUD = "1" }
# WOW_FPS_JOURNAL matches the engine's WOW_FPS_ prefix, which otherwise forces a
# 640x360 windowed probe (gxWindow ignored). WOW_BG=0 keeps a normal play window.
if (-not $env:WOW_BG) { $env:WOW_BG = "0" }
$log = Join-Path $diag "benilla-$stamp.log"

Set-Location $benilla
Write-Host "Launching benilla (release) at native desktop size"
Write-Host "Telemetry $stamp -> $diag"
Write-Host "  stream  $env:WOW_STREAM_TRACE"
Write-Host "  fps     $env:WOW_FPS_JOURNAL"
Write-Host "  pipe    $env:WOW_PIPE_TRACE"
Write-Host "  stall   WOW_STALL=$env:WOW_STALL (monitor only)"
Write-Host "  window  WOW_BG=$env:WOW_BG (0 = normal borderless play)"
Write-Host "  log     $log"
# PowerShell 5.1 + $ErrorActionPreference Stop treats cargo's stderr progress
# ("Finished", "Running") as NativeCommandError and aborts before the exe starts.
# Merge streams in cmd so Tee-Object only sees stdout.
$prevEap = $ErrorActionPreference
$ErrorActionPreference = "Continue"
try {
    & cmd.exe /c "cargo run --release -p benilla 2>&1" | Tee-Object -FilePath $log
    $code = $LASTEXITCODE
} finally {
    $ErrorActionPreference = $prevEap
}
if ($code -ne 0) {
    throw "benilla exited with code $code"
}
