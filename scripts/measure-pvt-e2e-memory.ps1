param(
    [int]$SampleIntervalMs = 500,
    [string]$OutputPath = ".scratch/pvt_e2e_memory.json",
    [string]$StdoutPath = ".scratch/pvt_e2e_memory_stdout.txt",
    [string]$StderrPath = ".scratch/pvt_e2e_memory_stderr.txt"
)

$ErrorActionPreference = "Stop"

function Get-ProcessTreeIds {
    param([int]$RootProcessId)

    $ids = New-Object System.Collections.Generic.List[int]
    $queue = New-Object System.Collections.Generic.Queue[int]
    $queue.Enqueue($RootProcessId)

    while ($queue.Count -gt 0) {
        $current = $queue.Dequeue()
        if ($ids.Contains($current)) {
            continue
        }
        $ids.Add($current)

        $children = Get-CimInstance Win32_Process -Filter "ParentProcessId = $current" -ErrorAction SilentlyContinue
        foreach ($child in $children) {
            $queue.Enqueue([int]$child.ProcessId)
        }
    }

    return $ids
}

function Get-TrackedProcessIds {
    param([int]$RootProcessId)

    $ids = New-Object System.Collections.Generic.List[int]
    foreach ($id in Get-ProcessTreeIds -RootProcessId $RootProcessId) {
        if (-not $ids.Contains($id)) {
            $ids.Add($id)
        }
    }

    $workspace = (Get-Location).Path.ToLowerInvariant()
    $processes = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue
    foreach ($proc in $processes) {
        $commandLine = ""
        if ($null -ne $proc.CommandLine) {
            $commandLine = $proc.CommandLine.ToLowerInvariant()
        }
        $executablePath = ""
        if ($null -ne $proc.ExecutablePath) {
            $executablePath = $proc.ExecutablePath.ToLowerInvariant()
        }
        $matchesWorkspace = $commandLine.Contains($workspace) -or $executablePath.Contains($workspace)
        $matchesTest = $commandLine.Contains("pvt_e2e") -or $executablePath.Contains("pvt_e2e")
        if (($matchesWorkspace -or $matchesTest) -and -not $ids.Contains([int]$proc.ProcessId)) {
            $ids.Add([int]$proc.ProcessId)
        }
    }

    $namedProcesses = Get-Process -ErrorAction SilentlyContinue | Where-Object {
        $_.ProcessName -eq "cargo" -or $_.ProcessName -like "pvt_e2e*"
    }
    foreach ($proc in $namedProcesses) {
        if (-not $ids.Contains([int]$proc.Id)) {
            $ids.Add([int]$proc.Id)
        }
    }

    return $ids
}

function Measure-ProcessTreeMemory {
    param([int]$RootProcessId)

    $workingSet = 0L
    $privateBytes = 0L
    $processCount = 0

    foreach ($id in Get-TrackedProcessIds -RootProcessId $RootProcessId) {
        $proc = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($null -eq $proc) {
            continue
        }
        $workingSet += [int64]$proc.WorkingSet64
        $privateBytes += [int64]$proc.PrivateMemorySize64
        $processCount += 1
    }

    [PSCustomObject]@{
        WorkingSetBytes = $workingSet
        PrivateBytes = $privateBytes
        ProcessCount = $processCount
    }
}

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $OutputPath) | Out-Null
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $StdoutPath) | Out-Null
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $StderrPath) | Out-Null

$commandFile = "cargo"
$commandArgs = @(
    "test",
    "-p",
    "gnss-core",
    "--test",
    "pvt_e2e",
    "test_pvt_e2e",
    "--",
    "--nocapture"
)

$startedAt = Get-Date
$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
$quotedStdoutPath = '"' + (Resolve-Path -LiteralPath (Split-Path -Parent $StdoutPath)).Path + "\" + (Split-Path -Leaf $StdoutPath) + '"'
$quotedStderrPath = '"' + (Resolve-Path -LiteralPath (Split-Path -Parent $StderrPath)).Path + "\" + (Split-Path -Leaf $StderrPath) + '"'
$commandLine = "$commandFile $($commandArgs -join ' ') > $quotedStdoutPath 2> $quotedStderrPath"
$startInfo = New-Object System.Diagnostics.ProcessStartInfo
$startInfo.FileName = $env:ComSpec
$startInfo.Arguments = "/d /s /c `"$commandLine`""
$startInfo.WorkingDirectory = (Get-Location).Path
$startInfo.UseShellExecute = $false
$startInfo.CreateNoWindow = $true
$process = [System.Diagnostics.Process]::Start($startInfo)

$samples = New-Object System.Collections.Generic.List[object]
$peakWorkingSet = 0L
$peakPrivateBytes = 0L

while (-not $process.HasExited) {
    $memory = Measure-ProcessTreeMemory -RootProcessId $process.Id
    $peakWorkingSet = [Math]::Max($peakWorkingSet, [int64]$memory.WorkingSetBytes)
    $peakPrivateBytes = [Math]::Max($peakPrivateBytes, [int64]$memory.PrivateBytes)
    $samples.Add([PSCustomObject]@{
        ElapsedMs = [int64]$stopwatch.ElapsedMilliseconds
        WorkingSetBytes = [int64]$memory.WorkingSetBytes
        PrivateBytes = [int64]$memory.PrivateBytes
        ProcessCount = [int]$memory.ProcessCount
    })
    Start-Sleep -Milliseconds $SampleIntervalMs
}

$process.WaitForExit()
$stopwatch.Stop()

$result = [PSCustomObject]@{
    Command = "$commandFile $($commandArgs -join ' ')"
    StartedAt = $startedAt.ToString("o")
    ExitCode = [int]$process.ExitCode
    ElapsedMs = [int64]$stopwatch.ElapsedMilliseconds
    PeakWorkingSetBytes = [int64]$peakWorkingSet
    PeakPrivateBytes = [int64]$peakPrivateBytes
    PeakWorkingSetMiB = [Math]::Round($peakWorkingSet / 1MB, 2)
    PeakPrivateMiB = [Math]::Round($peakPrivateBytes / 1MB, 2)
    SampleIntervalMs = [int]$SampleIntervalMs
    StdoutPath = $StdoutPath
    StderrPath = $StderrPath
    Samples = $samples
}

$result | ConvertTo-Json -Depth 4 | Set-Content -Path $OutputPath -Encoding UTF8

Write-Host "exit_code=$($result.ExitCode)"
Write-Host "elapsed_s=$([Math]::Round($result.ElapsedMs / 1000.0, 2))"
Write-Host "peak_working_set_mib=$($result.PeakWorkingSetMiB)"
Write-Host "peak_private_mib=$($result.PeakPrivateMiB)"
Write-Host "output=$OutputPath"
Write-Host "stdout=$StdoutPath"
Write-Host "stderr=$StderrPath"

exit $process.ExitCode
