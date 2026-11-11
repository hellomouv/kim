<#
.SYNOPSIS
  Milestone C recovery matrix (Step 31, docs/MILESTONE_C_RECOVERY_VALIDATION.md): phases run
  around a real Windows transition (lock/unlock, long idle, sleep/resume, hibernate/resume,
  reboot, power loss, Host or client crash).

.DESCRIPTION
  Uses its own endpoint and state folder, never the user's Subak or state:
    endpoint  subak-matrix-<user>
    state     %LOCALAPPDATA%\mouv\subak\matrix-state
    evidence  %LOCALAPPDATA%\mouv\subak\matrix-evidence\<timestamp>\
  Nothing here locks, sleeps, hibernates or reboots the PC: the maintainer does that between
  phases. No secret is printed; command lines are Subak's own redacted output.

  Phases:
    prepare      start a Host with a cmd, a PowerShell, a ticking job, a multi-pane Terminal;
                 record pids, topology and a tick count (before.json)
    check-live   after lock/unlock, idle, sleep or hibernate: the same Host and pane pids must
                 still run, the job must still tick, the Host must answer (after.json, verdict)
    check-cold   after a reboot or a power loss: the Host is gone, state.json must be valid,
                 `recover list` shows the Terminals, `recover --yes` rebuilds the topology with
                 new pids and without re-running the job twice (verdict, timings)
    cleanup      stop the matrix Host and delete its state

.EXAMPLE
  .\scripts\recovery-matrix.ps1 prepare
  (lock the PC, wait, unlock)
  .\scripts\recovery-matrix.ps1 check-live
  (reboot)
  .\scripts\recovery-matrix.ps1 check-cold
  .\scripts\recovery-matrix.ps1 cleanup
#>
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet('prepare', 'check-live', 'check-cold', 'cleanup')]
    [string]$Phase,
    [string]$Subak = (Join-Path $PSScriptRoot '..\target\release\subak.exe'),
    [string]$Transition = 'unspecified'
)
$ErrorActionPreference = 'Stop'
$ep = "subak-matrix-$env:USERNAME"
$state = Join-Path $env:LOCALAPPDATA 'mouv\subak\matrix-state'
$root = Join-Path $env:LOCALAPPDATA 'mouv\subak\matrix-evidence'
$current = Join-Path $root 'current.txt'

function J([string[]]$a) {
    $out = & $Subak --json --endpoint $ep @a
    $doc = $out | ConvertFrom-Json
    return $doc
}
function Need($doc, [string]$what) {
    if (-not $doc.ok) { throw "$what failed: $($doc.error.code) $($doc.error.message)" }
    return $doc.data
}
function Pids {
    $list = (Need (J @('ls')) 'ls').terminals
    $all = @()
    foreach ($t in $list) {
        $d = Need (J @('pane', 'ls', $t.id)) 'pane ls'
        foreach ($w in $d.windows) { foreach ($p in $w.panes) { if ($p.pid) { $all += [int]$p.pid } } }
    }
    return , $all
}
function Ticks {
    $lines = (Need (J @('capture', 'job')) 'capture').lines
    return @($lines | Where-Object { $_ -match '127\.0\.0\.1' }).Count
}
function Os { (Get-CimInstance Win32_OperatingSystem | ForEach-Object { "$($_.Caption) $($_.Version) build $($_.BuildNumber)" }) }

switch ($Phase) {
    'prepare' {
        $dir = Join-Path $root (Get-Date -Format 'yyyyMMdd-HHmmss')
        New-Item -ItemType Directory -Force $dir | Out-Null
        Set-Content -Encoding utf8 $current $dir
        Need (J @('host', 'start', '--no-shell', '--idle-exit', '86400', '--state-dir', $state)) 'host start' | Out-Null
        Need (J @('new', 'cmd')) 'new cmd' | Out-Null
        Need (J @('new', 'ps', '--', 'powershell.exe', '-NoLogo', '-NoProfile')) 'new ps' | Out-Null
        # A job that keeps printing (one line a second); no nested quotes (Windows PowerShell 5.1
        # would split them when passing them to a native program).
        Need (J @('new', 'job', '--', 'ping.exe', '-t', '127.0.0.1')) 'new job' | Out-Null
        Need (J @('new', 'multi')) 'new multi' | Out-Null
        Need (J @('command', '--target', 'multi', 'split-window -h; split-window -v; new-window -n second')) 'splits' | Out-Null
        Start-Sleep -Seconds 3
        $host_ = (Need (J @('host', 'status')) 'host status').host
        $before = [ordered]@{
            phase = 'prepare'; at = (Get-Date).ToUniversalTime().ToString('o'); os = (Os)
            host_pid = $host_.pid; pane_pids = (Pids); ticks = (Ticks)
            topology = (Need (J @('pane', 'ls', 'multi')) 'pane ls multi')
        }
        $before | ConvertTo-Json -Depth 30 | Set-Content -Encoding utf8 (Join-Path $dir 'before.json')
        "prepared: Host pid $($host_.pid), $($before.pane_pids.Count) panes, evidence in $dir"
        'Now do the transition (lock/unlock, sleep, hibernate, reboot, ...), then run check-live or check-cold.'
    }
    'check-live' {
        $dir = Get-Content $current
        $before = Get-Content (Join-Path $dir 'before.json') -Raw | ConvertFrom-Json
        $t0 = Get-Date
        $h = J @('host', 'health')
        $ms = [int]((Get-Date) - $t0).TotalMilliseconds
        $ok = $h.ok -and ($h.data.host.pid -eq $before.host_pid)
        $now = if ($h.ok) { Pids } else { @() }
        $same = $ok -and (@(Compare-Object $now $before.pane_pids).Count -eq 0)
        $ticks1 = if ($h.ok) { Ticks } else { 0 }
        Start-Sleep -Seconds 3
        $ticks2 = if ($h.ok) { Ticks } else { 0 }
        $verdict = [ordered]@{
            phase = 'check-live'; transition = $Transition; at = (Get-Date).ToUniversalTime().ToString('o'); os = (Os)
            host_answers = $h.ok; host_health_ms = $ms; same_host = $ok; same_pane_pids = $same
            job_ticked_during_transition = ($ticks1 -gt $before.ticks); job_ticks_now = ($ticks2 -gt $ticks1)
            pass = ($ok -and $same -and ($ticks2 -gt $ticks1))
        }
        $verdict | ConvertTo-Json -Depth 10 | Set-Content -Encoding utf8 (Join-Path $dir "check-live-$Transition.json")
        $verdict | ConvertTo-Json -Depth 10
    }
    'check-cold' {
        $dir = Get-Content $current
        $before = Get-Content (Join-Path $dir 'before.json') -Raw | ConvertFrom-Json
        $wasRunning = (J @('host', 'status')).ok
        if (-not $wasRunning) {
            Need (J @('host', 'start', '--no-shell', '--idle-exit', '86400', '--state-dir', $state)) 'host start' | Out-Null
        }
        $check = & $Subak --json state check --state-dir $state | ConvertFrom-Json
        $list = Need (J @('recover', 'list')) 'recover list'
        $t0 = Get-Date
        $failed = @()
        foreach ($t in $list.terminals) {
            $r = J @('recover', $t.name, '--yes')
            if (-not $r.ok) { $failed += "$($t.name): $($r.error.code)" }
            elseif ($r.data.failed.Count -gt 0) { $failed += $r.data.failed }
        }
        $ms = [int]((Get-Date) - $t0).TotalMilliseconds
        $again = J @('recover', 'multi', '--yes')
        $topo = Need (J @('pane', 'ls', 'multi')) 'pane ls multi'
        $now = Pids
        $verdict = [ordered]@{
            phase = 'check-cold'; transition = $Transition; at = (Get-Date).ToUniversalTime().ToString('o'); os = (Os)
            host_survived = $wasRunning; state_valid = $check.ok; recoverable = $list.terminals.Count
            recover_ms = $ms; failed = $failed
            same_terminal_id = ($topo.terminal_id -eq $before.topology.terminal_id)
            same_windows = ($topo.windows.Count -eq $before.topology.windows.Count)
            same_panes = ((@($topo.windows | ForEach-Object { $_.panes.Count }) -join ',') -eq (@($before.topology.windows | ForEach-Object { $_.panes.Count }) -join ','))
            new_pids = (@($now | Where-Object { $before.pane_pids -contains $_ }).Count -eq 0)
            second_recover_refused = (-not $again.ok)
        }
        $verdict.pass = (-not $wasRunning) -and $check.ok -and ($failed.Count -eq 0) -and $verdict.same_terminal_id -and $verdict.same_windows -and $verdict.same_panes -and $verdict.new_pids -and $verdict.second_recover_refused
        $verdict | ConvertTo-Json -Depth 10 | Set-Content -Encoding utf8 (Join-Path $dir "check-cold-$Transition.json")
        $verdict | ConvertTo-Json -Depth 10
    }
    'cleanup' {
        & $Subak --endpoint $ep host stop --kill-terminals --yes 2>$null | Out-Null
        Start-Sleep -Seconds 1
        if (Test-Path $state) { Remove-Item -Recurse -Force $state }
        'matrix Host stopped and its state removed (evidence kept in ' + $root + ')'
    }
}
