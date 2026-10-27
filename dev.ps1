param(
    [Parameter(Position = 0)]
    [ValidateSet("editor", "admin", "game", "workspace", "all", "launcher", "vulkan-probe", "check")]
    [string]$Project = "launcher",
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$launcherPath = Join-Path $PSScriptRoot "launcher.py"
$launchActions = @{
    editor = "client"
    admin = "dashboard"
    game = "engine"
    workspace = "workspace"
    all = "all"
    "vulkan-probe" = "vulkan-probe"
    check = "check"
}

$pythonCommand = Get-Command python -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
$pythonArguments = @()
if (!$pythonCommand)
{
    $pythonCommand = Get-Command py -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    $pythonArguments += "-3"
}
if (!$pythonCommand)
{
    throw "Python 3 is unavailable. Install it and add python or py to PATH."
}

# Keep all project commands in the shared launcher, including their working directories.
$pythonArguments += @($launcherPath, "--no-pause")
if ($Project -ne "launcher")
{
    $pythonArguments += @("--run", $launchActions[$Project])
}
if ($DryRun)
{
    $pythonArguments += "--dry-run"
}

& $pythonCommand.Source @pythonArguments
exit $LASTEXITCODE
