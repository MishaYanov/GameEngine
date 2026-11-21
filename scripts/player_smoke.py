"""Run the native Player and require its stable first-frame marker."""

import os
import subprocess


FRAME_MARKER = b"GAME_STARTUP_SMOKE_OK mode=player"
SMOKE_TIMEOUT_SECONDS = 90


def run_logged(command, cwd, log, timeout, *, env=None, creationflags=0):
    with log.open("xb") as output:
        try:
            subprocess.run(
                command,
                cwd=cwd,
                env=env,
                creationflags=creationflags,
                stdout=output,
                stderr=subprocess.STDOUT,
                timeout=timeout,
                check=True,
            )
        except subprocess.CalledProcessError as error:
            raise RuntimeError(f"{log.stem} exited with code {error.returncode}. See {log}") from error
        except subprocess.TimeoutExpired as error:
            raise RuntimeError(f"{log.stem} timed out after {timeout} seconds. See {log}") from error
        except OSError as error:
            raise RuntimeError(f"Could not start {log.stem}: {error}. See {log}") from error


def smoke(player, cwd, log, *options, profile_home=None):
    environment = {
        **os.environ,
        "ENGINE_LOG_DIR": str(log.parent / "native-logs"),
    }
    environment.pop("GAME_ENGINE_NATIVE_EDITOR_DEMO", None)
    if profile_home is not None:
        environment["LOCALAPPDATA"] = str(profile_home)
    run_logged(
        [str(player), "--smoke-test", *options],
        cwd,
        log,
        SMOKE_TIMEOUT_SECONDS,
        env=environment,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )
    with log.open("rb") as output:
        if FRAME_MARKER not in output.read(4 * 1024 * 1024).splitlines():
            raise RuntimeError(f"Player frame marker absent. See {log}")
