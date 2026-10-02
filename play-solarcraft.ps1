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
Set-Location $benilla
Write-Host "Launching benilla (release) at native desktop size"
cargo run --release -p benilla
