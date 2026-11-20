param(
    [string]$OutputDirectory = 'target/performance',
    [switch]$EnforceBudget
)
$ErrorActionPreference = 'Stop'
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$outputRoot = [System.IO.Path]::GetFullPath((Join-Path $repoRoot $OutputDirectory))
New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
Push-Location $repoRoot
try {
    & cargo build --release -p engine-core --example performance_baseline
    if ($LASTEXITCODE -ne 0) { throw 'Failed to build the CPU benchmark.' }
    $executable = Join-Path $repoRoot 'target/release/examples/performance_baseline.exe'
    $results = @()
    for ($run = 1; $run -le 5; $run++) {
        $jsonPath = Join-Path $outputRoot "core-run-$run.json"
        $scenePath = Join-Path $outputRoot 'representative-scene.json'
        # Explicit literal quoting handles the workspace's spaces; no shell executes these paths.
        $argumentList = @('--output', ('"' + $jsonPath + '"'), '--scene-output', ('"' + $scenePath + '"'))
        $benchmark = Start-Process -FilePath $executable -ArgumentList $argumentList -WorkingDirectory $repoRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $outputRoot "core-run-$run.stdout.log") -RedirectStandardError (Join-Path $outputRoot "core-run-$run.stderr.log")
        $peakWorkingSet = 0L
        while (-not $benchmark.HasExited) {
            $benchmark.Refresh()
            $peakWorkingSet = [Math]::Max($peakWorkingSet, $benchmark.PeakWorkingSet64)
            Start-Sleep -Milliseconds 5
        }
        $benchmark.WaitForExit()
        if ($benchmark.ExitCode -ne 0) { throw "CPU benchmark run $run failed with exit code $($benchmark.ExitCode)." }
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
