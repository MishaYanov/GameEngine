[CmdletBinding()]
param(
    [string]$OutputDirectory,
    [string]$CargoTargetDirectory,
    [switch]$PortableOnly,
    [switch]$Offline
)

$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT)
{
    throw 'Build the Windows Editor on Windows with the x64 MSVC Rust toolchain.'
}

function Invoke-Checked
{
    param([string]$Program, [string[]]$CommandArguments)
    & $Program @CommandArguments
    if ($LASTEXITCODE -ne 0)
    {
        throw "$Program exited with status $LASTEXITCODE."
    }
}

function Assert-CopiedFile
{
    param([string]$ExpectedFile, [string]$CopiedFile)
    if ((Get-FileHash -LiteralPath $ExpectedFile).Hash -ne (Get-FileHash -LiteralPath $CopiedFile).Hash)
    {
        throw "The packaged copy differs from the validated source: $CopiedFile"
    }
}

function Assert-EmbeddedFrontend
{
    param([string]$BrowserDirectory, [string]$Executable)
    $entry = @(Get-ChildItem -LiteralPath $BrowserDirectory -File -Filter 'main-*.js')
    if (!(Test-Path -LiteralPath (Join-Path $BrowserDirectory 'index.html') -PathType Leaf) -or $entry.Count -ne 1)
    {
        throw "Expected an Angular index and entry bundle in $BrowserDirectory"
    }
    # Tauri keeps embedded asset keys as plain strings even when it compresses the contents.
    $binary = [Text.Encoding]::GetEncoding(28591).GetString([IO.File]::ReadAllBytes($Executable))
    if ($binary.IndexOf($entry[0].Name, [StringComparison]::Ordinal) -lt 0)
    {
        throw "The frontend entry bundle is missing from $Executable"
    }
}

$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$targetTriple = 'x86_64-pc-windows-msvc'
$editorName = 'game-engine-editor'
if (!$OutputDirectory)
{
    $run = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N').Substring(0, 8)
    $OutputDirectory = Join-Path $root "target\windows-editor-release\$run"
}
if (!$CargoTargetDirectory)
{
    $CargoTargetDirectory = Join-Path $root 'target\windows-editor-cargo'
}
$output = [IO.Path]::GetFullPath($OutputDirectory)
$cargoTarget = [IO.Path]::GetFullPath($CargoTargetDirectory)
if (Test-Path -LiteralPath $output)
{
    throw "Choose a new release output directory: $output"
}

$frontend = Join-Path $root 'apps/editor'
$cli = Join-Path $frontend 'node_modules/@tauri-apps/cli/tauri.js'
if (!(Test-Path -LiteralPath $cli -PathType Leaf))
{
    throw 'Install locked frontend dependencies with npm ci in apps/editor before building the release.'
}
if (!$PortableOnly)
{
    $dashboard = Join-Path $root 'apps/admin-dashboard'
    $dashboardCli = Join-Path $dashboard 'node_modules/@tauri-apps/cli/tauri.js'
    if (!(Test-Path -LiteralPath $dashboardCli -PathType Leaf))
    {
        throw 'Install locked frontend dependencies with npm ci in apps/admin-dashboard before building the installer.'
    }
}

[IO.Directory]::CreateDirectory($output) | Out-Null
$previousTarget = $env:CARGO_TARGET_DIR
$previousOffline = $env:CARGO_NET_OFFLINE
$previousDashboardExe = $env:GAME_ENGINE_ADMIN_BUNDLE_EXE
$env:CARGO_TARGET_DIR = $cargoTarget
if ($Offline)
{
    # This setting controls Cargo. Tauri can provision its own installer tools.
    $env:CARGO_NET_OFFLINE = 'true'
}

Push-Location -LiteralPath $root
try
{
    Invoke-Checked 'cargo' @('build', '--locked', '--release', '--target', $targetTriple, '-p', 'game', '-p', 'compiler', '-p', 'authoring-agent-worker')
    $buildDirectory = Join-Path $cargoTarget "$targetTriple\release"
    $player = Join-Path $buildDirectory 'game.exe'
    $agentFiles = [ordered]@{
        'authoring-agent-worker.exe' = Join-Path $buildDirectory 'authoring-agent-worker.exe'
        'bundle.json' = Join-Path $root 'crates/authoring-agent/profiles/game-development-support.json'
        'engine-help/editor-workspace.md' = Join-Path $root 'docs/editor-workspace.md'
        'engine-help/authoring-objects.md' = Join-Path $root 'docs/authoring-objects.md'
        'engine-help/scene-editing.md' = Join-Path $root 'docs/scene-editing.md'
        'engine-help/project-lifecycle.md' = Join-Path $root 'docs/project-lifecycle.md'
    }

    $sampleManifest = Join-Path $root 'projects\cinder-oath\project.json'
    $sampleStaging = Join-Path $output 'sample-staging'
    [IO.Directory]::CreateDirectory($sampleStaging) | Out-Null
    Invoke-Checked (Join-Path $buildDirectory 'compiler.exe') @('--stage', $sampleManifest, $sampleStaging)
    $sampleTitle = (Get-Content -Raw -LiteralPath $sampleManifest | ConvertFrom-Json).name
    $sample = Join-Path $sampleStaging $sampleTitle

    $frontendOutput = Join-Path $output 'frontend'
    Push-Location -LiteralPath $frontend
    try
    {
        Invoke-Checked 'npm.cmd' @('run', 'build', '--', '--output-path', $frontendOutput)
    }
    finally
    {
        Pop-Location
    }

    $resources = @{ }
    $resources[$player.Replace('\', '/')] = 'release/player/game.exe'
    $resources[$sample.Replace('\', '/')] = 'release/samples/cinder-oath'
    foreach ($relative in $agentFiles.Keys)
    {
        $agentFile = $agentFiles[$relative]
        if (!(Test-Path -LiteralPath $agentFile -PathType Leaf))
        {
            throw "The Agent release file is missing: $agentFile"
        }
        $resources[$agentFile.Replace('\', '/')] = "release/agent/$relative"
    }
    $overlay = Join-Path $output 'tauri-release.json'
    $config = @{
        mainBinaryName = $editorName
        build = @{
            beforeBuildCommand = $null
            # A single Windows drive path can be parsed as a URL. The list embeds local assets.
            frontendDist = @((Join-Path $frontendOutput 'browser').Replace('\', '/'))
        }
        bundle = @{
            resources = $resources
            useLocalToolsDir = $true
        }
    }

    # Enable the Editor feature that selects the bundled Player and sample.
    $bundleArguments = @('build', '--ci', '--target', $targetTriple, '--config', $overlay, '--features', 'custom-protocol')
    if ($PortableOnly)
    {
        $bundleArguments += '--no-bundle'
    }
    else
    {
        $dashboardFrontend = Join-Path $output 'dashboard-frontend'
        $dashboardOverlay = Join-Path $output 'tauri-dashboard-release.json'
        $dashboardConfig = @{
            build = @{
                beforeBuildCommand = $null
                frontendDist = @((Join-Path $dashboardFrontend 'browser').Replace('\', '/'))
            }
        } | ConvertTo-Json -Depth 5
        [IO.File]::WriteAllText($dashboardOverlay, $dashboardConfig,[Text.UTF8Encoding]::new($false))
        Push-Location -LiteralPath $dashboard
        try
        {
            Invoke-Checked 'npm.cmd' @('run', 'build', '--', '--output-path', $dashboardFrontend)
            Invoke-Checked 'node' @($dashboardCli, 'build', '--ci', '--target', $targetTriple, '--config', $dashboardOverlay,
            '--features', 'desktop', '--no-bundle', '--', '--locked')
        }
        finally
        {
            Pop-Location
        }
        $dashboardExe = Join-Path $buildDirectory 'engine-admin.exe'
        if (!(Test-Path -LiteralPath $dashboardExe -PathType Leaf))
        {
            throw 'The Admin Dashboard release executable was not built.'
        }
        Assert-EmbeddedFrontend (Join-Path $dashboardFrontend 'browser') $dashboardExe
        $env:GAME_ENGINE_ADMIN_BUNDLE_EXE = $dashboardExe
        $config.bundle.windows = @{
            nsis = @{ installerHooks = (Join-Path $PSScriptRoot 'installer-hooks.nsh').Replace('\', '/') }
        }
        $bundleArguments += @('--bundles', 'nsis')
    }
    $bundleArguments += @('--', '--locked')
    [IO.File]::WriteAllText($overlay, ($config | ConvertTo-Json -Depth 7),[Text.UTF8Encoding]::new($false))
    Push-Location -LiteralPath $PSScriptRoot
    try
    {
        Invoke-Checked 'node' (@($cli) + $bundleArguments)
    }
    finally
    {
        Pop-Location
    }

    $editorExe = Join-Path $buildDirectory "$editorName.exe"
    Assert-EmbeddedFrontend (Join-Path $frontendOutput 'browser') $editorExe
    $builtResources = Join-Path $buildDirectory 'release'
    Assert-CopiedFile $player (Join-Path $builtResources 'player\game.exe')
    foreach ($relative in $agentFiles.Keys)
    {
        Assert-CopiedFile $agentFiles[$relative] (Join-Path $builtResources "agent/$relative")
    }
    foreach ($file in Get-ChildItem -LiteralPath $sample -File -Recurse)
    {
        $relative = $file.FullName.Substring($sample.Length + 1)
        Assert-CopiedFile $file.FullName (Join-Path $builtResources "samples\cinder-oath\$relative")
    }

    $portable = Join-Path $output 'Editor'
    $portablePlayer = Join-Path $portable 'release/player'
    $portableSamples = Join-Path $portable 'release/samples'
    [IO.Directory]::CreateDirectory($portablePlayer) | Out-Null
    [IO.Directory]::CreateDirectory($portableSamples) | Out-Null
    Copy-Item -LiteralPath $editorExe -Destination $portable
    Copy-Item -LiteralPath $player -Destination $portablePlayer
    foreach ($relative in $agentFiles.Keys)
    {
        $destination = Join-Path $portable "release/agent/$relative"
        [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination)) | Out-Null
        Copy-Item -LiteralPath $agentFiles[$relative] -Destination $destination
        Assert-CopiedFile $agentFiles[$relative] $destination
    }
    Copy-Item -LiteralPath $sample -Destination (Join-Path $portableSamples 'cinder-oath') -Recurse
    Write-Output "Windows Editor: $portable"

    if (!$PortableOnly)
    {
        $version = (Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'src-tauri\tauri.conf.json') | ConvertFrom-Json).version
        $installerDirectory = Join-Path $buildDirectory 'bundle\nsis'
        $installers = @(Get-ChildItem -LiteralPath $installerDirectory -File -Filter "*_${version}_x64-setup.exe")
        if ($installers.Count -ne 1)
        {
            throw 'Expected one x64 Windows Editor installer from the Tauri NSIS build.'
        }
        $installer = Join-Path $output $installers[0].Name
        Copy-Item -LiteralPath $installers[0].FullName -Destination $installer
        Write-Output "Windows Editor installer: $installer"
    }
}
finally
{
    Pop-Location
    $env:CARGO_TARGET_DIR = $previousTarget
    $env:CARGO_NET_OFFLINE = $previousOffline
    $env:GAME_ENGINE_ADMIN_BUNDLE_EXE = $previousDashboardExe
}
