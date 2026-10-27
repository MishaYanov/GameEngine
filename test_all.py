"""Run repository test suites, preserving output and reporting every failure."""
import argparse
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent


def suites(native=False):
	cargo = shutil.which("cargo") or "cargo"
	node = shutil.which("node") or "node"
	rust = [cargo, "test", "--workspace", "--all-targets", "--offline", "--locked"]
	commands = [
		("Rust workspace", ROOT, rust),
		("Rust documentation", ROOT, [cargo, "test", "--workspace", "--doc", "--offline", "--locked"]),
		("Admin Angular", ROOT / "apps/admin-dashboard", [node, "node_modules/vitest/vitest.mjs", "run"]),
		("Client Angular", ROOT / "apps/editor", [node, "node_modules/@angular/cli/bin/ng.js", "test", "editor", "--watch=false"]),
		("Python tools", ROOT, [sys.executable, "-m", "unittest", "discover", "-s", "tests", "-p", "test_*.py", "-v"]),
	]
	if native:
		commands.append(("Native Vulkan runtime", ROOT, [cargo, "test", "-p", "runtime", "--offline", "--locked", "native_runtime_initializes_and_reports_consistent_status", "--", "--ignored", "--nocapture"]))
		commands.append(("Native generated game", ROOT, [cargo, "test", "-p", "engine-admin", "--offline", "--locked", "real_native_template_validation", "--", "--ignored", "--nocapture"]))
		commands.append(("Native scene rendering", ROOT, [cargo, "run", "-p", "game", "--example", "renderer_smoke", "--offline", "--locked"]))
		commands.append(("Native scene save/reload", ROOT, [cargo, "run", "-p", "game", "--example", "scene_io_smoke", "--offline", "--locked"]))
	return commands


def run_suites(commands):
	results = []
	for name, directory, command in commands:
		print(f"\n=== {name} ===", flush=True)
		try:
			code = subprocess.run(command, cwd=directory, check=False).returncode
		except OSError as error:
			print(f"Could not run {name}: {error}", flush=True)
			code = 1
		results.append((name, code))
	print("\nTest summary", flush=True)
	for name, code in results:
		print(f"{'PASS' if code == 0 else 'FAIL'}: {name} (exit {code})", flush=True)
	return int(any(code != 0 for _, code in results))


def main(argv=None):
	parser = argparse.ArgumentParser(description=__doc__)
	parser.add_argument("--native", action="store_true", help="Also initialize Vulkan and build a generated game; requires Vulkan GPU, SDK and cached dependencies")
	args = parser.parse_args(argv)
	try:
		return run_suites(suites(args.native))
	except KeyboardInterrupt:
		print("\nTests interrupted.", flush=True)
		return 130


if __name__ == "__main__":
	raise SystemExit(main())
