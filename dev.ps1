param(
    [Parameter(Position = 0)]
    [ValidateSet(
        "editor",
        "game",
        "vulkan-probe"
    )]
    [string]$Project = "editor"
)

$Root = $PSScriptRoot

function Run-InDirectory {
    param(
        [string]$Path,
        [scriptblock]$Command
    )

    Push-Location $Path

    try {
        & $Command

        if ($LASTEXITCODE -ne 0) {
            throw "Command failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

switch ($Project) {

    "editor" {
        Write-Host "Starting Game Engine Editor..."

        Run-InDirectory "$Root\apps\editor" {
            npx tauri dev
        }
    }

    "game" {
        Write-Host "Starting Game..."

        Run-InDirectory $Root {
            cargo run -p game
        }
    }

    "vulkan-probe" {
        Write-Host "Starting Vulkan probe..."

        Run-InDirectory $Root {
            cargo run -p renderer --example vulkan_probe
        }
    }
}