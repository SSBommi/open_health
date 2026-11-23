# Battery test helper for the Oura ring (Windows PowerShell 5.1+).
#
# Usage, from the open_health folder:
#   powershell -ExecutionPolicy Bypass -File scripts\battery.ps1 check
#   powershell -ExecutionPolicy Bypass -File scripts\battery.ps1 stage1
#
# Actions:
#   check   Read the battery, log it to data\battery-log.csv, print the drain
#           per day since the last check on the same stage.
#   stage1  Workout HR off. Daytime HR, SpO2, resting HR, steps on.
#   stage2  Stage 1 + SpO2 off.
#   stage3  Stage 2 + daytime HR off (night HR, HRV, temp, sleep still recorded).
#   all-on  Everything back on.
#
# Take the ring OFF the charger before "check": readings on the dock run high.

param(
    [Parameter(Position = 0)]
    [ValidateSet("check", "stage1", "stage2", "stage3", "all-on")]
    [string]$Action = "check"
)

# "Continue": Windows PowerShell 5.1 turns a native tool's stderr into a
# terminating error under "Stop", and oura.exe reports failures on stderr.
$ErrorActionPreference = "Continue"
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root "target\release\oura.exe"
$key = Join-Path $root "data\ring.key"
$log = Join-Path $root "data\battery-log.csv"
$stageFile = Join-Path $root "data\battery-stage.txt"

if (-not (Test-Path $exe)) { Write-Host "oura.exe not found. Run 'cargo build --release' first."; exit 1 }
if (-not (Test-Path $key)) { Write-Host "Ring key not found at $key."; exit 1 }

function Set-Feature([string]$feature, [string]$mode) {
    # The ring sometimes misses one scan right after the previous connection closes,
    # so try up to three times with a short pause.
    for ($try = 1; $try -le 3; $try++) {
        $out = & $exe --key-file $key feature-mode $feature --mode $mode 2>&1 | Out-String
        if ($out -match "SUCCESS") {
            Write-Host ("  {0,-12} {1}" -f $feature, $mode)
            return
        }
        Start-Sleep -Seconds 3
    }
    $why = ($out -split "`n" | Where-Object { $_ -match "Error:|Caused by|no matching|ble error" } | ForEach-Object { $_.Trim() }) -join " "
    Write-Host ("  {0,-12} FAILED after 3 tries: {1}" -f $feature, $why) -ForegroundColor Yellow
}

function Get-Stage {
    if (Test-Path $stageFile) { return (Get-Content $stageFile -Raw).Trim() }
    return "unknown"
}

# feature ids: 0x02 daytime HR, 0x04 SpO2, 0x08 resting HR
$plans = @{
    "all-on" = @{ "exercise_hr" = "automatic"; "0x04" = "automatic"; "0x02" = "automatic"; "0x08" = "automatic"; "real_steps" = "automatic" }
    "stage1" = @{ "exercise_hr" = "off";       "0x04" = "automatic"; "0x02" = "automatic"; "0x08" = "automatic"; "real_steps" = "automatic" }
    "stage2" = @{ "exercise_hr" = "off";       "0x04" = "off";       "0x02" = "automatic"; "0x08" = "automatic"; "real_steps" = "automatic" }
    "stage3" = @{ "exercise_hr" = "off";       "0x04" = "off";       "0x02" = "off";       "0x08" = "automatic"; "real_steps" = "automatic" }
}

if ($Action -ne "check") {
    Write-Host "Applying $Action ..."
    foreach ($f in @("exercise_hr", "0x04", "0x02", "0x08", "real_steps")) {
        Set-Feature $f $plans[$Action][$f]
    }
    Set-Content -Path $stageFile -Value $Action
    Write-Host "Done. Now run 'check' to log a starting battery reading."
    exit 0
}

# --- check ---
$info = & $exe --key-file $key info 2>&1 | Out-String
if ($info -match "Battery\s*:\s*(\d+)%") {
    $pct = [int]$Matches[1]
} else {
    Write-Host "Could not read the battery. Is the ring nearby? Output:`n$info" -ForegroundColor Yellow
    exit 1
}
$now = Get-Date
$stage = Get-Stage

if (-not (Test-Path $log)) { "time,battery_pct,stage" | Set-Content $log }
$prev = Import-Csv $log | Where-Object { $_.stage -eq $stage } | Select-Object -Last 1
"{0},{1},{2}" -f $now.ToString("s"), $pct, $stage | Add-Content $log

Write-Host ("Battery {0}%  (stage: {1})" -f $pct, $stage)
if ($prev) {
    $hours = ($now - [datetime]$prev.time).TotalHours
    $drop = [int]$prev.battery_pct - $pct
    if ($drop -lt 0) {
        Write-Host "Battery went up since the last check (charged). Starting a fresh measurement from here."
    } elseif ($hours -lt 2) {
        Write-Host ("Only {0:N1} h since the last check; wait a few hours for a useful number." -f $hours)
    } else {
        $perDay = $drop / $hours * 24
        Write-Host ("Dropped {0}% in {1:N1} h  =  about {2:N0}% per day  (~{3:N1} days per charge)" -f $drop, $hours, $perDay, (100 / [math]::Max($perDay, 0.1)))
        if ($perDay -gt 20) { Write-Host "Still high. Try the next stage." -ForegroundColor Yellow }
        else { Write-Host "That is in the normal range." -ForegroundColor Green }
    }
} else {
    Write-Host "First reading for this stage. Run 'check' again in a few hours."
}
