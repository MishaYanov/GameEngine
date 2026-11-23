param(
    [string]$OutputDirectory = 'target/performance',
    [switch]$EnforceBudget,
    [switch]$Offline
)
$ErrorActionPreference = 'Stop'
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$outputRoot = if ([System.IO.Path]::IsPathRooted($OutputDirectory)) {
    [System.IO.Path]::GetFullPath($OutputDirectory)
} else {
    [System.IO.Path]::GetFullPath((Join-Path $repoRoot $OutputDirectory))
}
New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
Push-Location $repoRoot
try {
    $buildArgs = @('build', '--release', '--locked', '-p', 'engine-core', '--example', 'performance_baseline', '--message-format=json-render-diagnostics')
    if ($Offline) { $buildArgs += '--offline' }
    $messages = & cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { throw 'Failed to build the CPU benchmark.' }

    $executables = @(
        foreach ($line in $messages) {
            $message = ConvertFrom-Json -InputObject $line
            if ($message.reason -eq 'compiler-artifact' -and $message.target.name -eq 'performance_baseline' -and
                $message.target.kind -contains 'example' -and $message.executable) {
                $message.executable
            }
        }
    )
    if ($executables.Count -ne 1 -or -not (Test-Path -LiteralPath $executables[0] -PathType Leaf)) {
        throw 'Cargo did not report the CPU benchmark executable.'
    }
    $executable = $executables[0]
    $results = @()
    for ($run = 1; $run -le 5; $run++) {
        $jsonPath = Join-Path $outputRoot "core-run-$run.json"
        $scenePath = Join-Path $outputRoot 'representative-scene.json'
        $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
        $startInfo.FileName = $executable
        $startInfo.WorkingDirectory = $repoRoot
        $startInfo.Arguments = '--output "{0}" --scene-output "{1}"' -f $jsonPath, $scenePath
        $startInfo.UseShellExecute = $false
        $startInfo.CreateNoWindow = $true
        $startInfo.RedirectStandardOutput = $true
        $startInfo.RedirectStandardError = $true

        # Keep the launch handle open so a short-lived child still has a readable exit code.
        $benchmark = [System.Diagnostics.Process]::Start($startInfo)
        try {
            $stdout = $benchmark.StandardOutput.ReadToEndAsync()
            $stderr = $benchmark.StandardError.ReadToEndAsync()
            $peakWorkingSet = 0L
            while (-not $benchmark.HasExited) {
                $benchmark.Refresh()
                try {
                    $peakWorkingSet = [Math]::Max($peakWorkingSet, $benchmark.PeakWorkingSet64)
                } catch [System.InvalidOperationException] {
                    if (-not $benchmark.HasExited) { throw }
                }
                Start-Sleep -Milliseconds 5
            }
            $benchmark.WaitForExit()
            [System.IO.File]::WriteAllText((Join-Path $outputRoot "core-run-$run.stdout.log"), $stdout.GetAwaiter().GetResult())
            [System.IO.File]::WriteAllText((Join-Path $outputRoot "core-run-$run.stderr.log"), $stderr.GetAwaiter().GetResult())
            if ($benchmark.ExitCode -ne 0) { throw "CPU benchmark run $run failed with exit code $($benchmark.ExitCode)." }
        } finally {
            $benchmark.Dispose()
        }
        $metrics = Get-Content -LiteralPath $jsonPath -Raw | ConvertFrom-Json
        $results += [pscustomobject]@{ run = $run; peak_working_set_bytes = $peakWorkingSet; metrics = $metrics }
    }
    $budgets = @{ load_p95_ms = 10.0; tick_p95_ms = 0.5; peak_working_set_bytes = 134217728 }
    $violations = @()
    foreach ($result in $results) {
        if ($result.metrics.load_p95_ms -gt $budgets.load_p95_ms) { $violations += "Run $($result.run): CPU load p95 exceeded 10ms." }
        if ($result.metrics.tick_p95_ms -gt $budgets.tick_p95_ms) { $violations += "Run $($result.run): simulation p95 exceeded 0.5ms." }
        if ($result.peak_working_set_bytes -eq 0) { $violations += "Run $($result.run): process memory was not observed." }
        elseif ($result.peak_working_set_bytes -gt $budgets.peak_working_set_bytes) { $violations += "Run $($result.run): process working set exceeded 128MiB." }
        if ($result.metrics.collision_rebuilds -ne 0) { $violations += "Run $($result.run): steady simulation rebuilt the collision cache." }
    }
    $report = [ordered]@{
        schema_version = 1
        measured_utc = [DateTime]::UtcNow.ToString('o')
        os = [Environment]::OSVersion.VersionString
        processor = $env:PROCESSOR_IDENTIFIER
        logical_processors = [Environment]::ProcessorCount
        memory_scope = 'Windows process peak working set sampled from the child handle; includes executable/runtime allocations, excludes GPU memory.'
        budgets = $budgets
        violations = @($violations)
        runs = @($results)
    }
    $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $outputRoot 'core-series.json') -Encoding utf8
    Write-Output "Measured five release runs. Report: $outputRoot/core-series.json"
    if ($violations.Count -gt 0) {
        $violations | Write-Warning
        if ($EnforceBudget) { throw 'Performance budgets were exceeded. Inspect core-series.json.' }
    }
} finally {
    Pop-Location
}
