"""Sign a current local release review with an explicitly selected OpenSSH key."""

import argparse
import os
from pathlib import Path
import stat
import subprocess
from zipfile import BadZipFile

from rehearse_release import ordinary_mode
from review_candidate import _write_new, build_packet, verified_decision_bytes

NAMESPACE = "game-engine-windows-release"
MAX_SIGNATURE_BYTES = 16 * 1024
SIGNATURE_HEADER = b"-----BEGIN SSH SIGNATURE-----"
SIGNATURE_FOOTER = b"-----END SSH SIGNATURE-----"


def add_inputs(parser):
    for name in ("receipt", "archive", "evidence_dir", "packet", "decision", "signature"):
        parser.add_argument(name, type=Path)
    parser.add_argument("--profiles", type=Path, required=True)
    parser.add_argument("--playtest", type=Path)
    parser.add_argument("--ssh-keygen", type=Path, help="Absolute path to a trusted OpenSSH executable")


def checked_decision(args):
    packet = build_packet(args.receipt, args.archive, args.evidence_dir, args.profiles,
                          playtest=args.playtest)
    return verified_decision_bytes(packet, args.packet, args.decision)


def system_ssh_keygen_paths():
    if os.name == "nt":
        windows = Path(os.environ.get("SystemRoot", ""))
        return (windows / "System32" / "OpenSSH" / "ssh-keygen.exe",) if windows.is_absolute() else ()
    return (Path("/usr/bin/ssh-keygen"), Path("/bin/ssh-keygen"))


def ssh_keygen(explicit=None):
    candidates = (explicit,) if explicit is not None else system_ssh_keygen_paths()
    for path in candidates:
        if (path.is_absolute() and (os.name != "nt" or path.suffix.lower() == ".exe")
                and path.is_file() and stat.S_ISREG(ordinary_mode(path)) and os.access(path, os.X_OK)):
            return str(path.resolve(strict=True))
    raise ValueError("OpenSSH ssh-keygen requires a system installation or an absolute trusted --ssh-keygen path")


def run_ssh(executable, arguments, payload):
    environment = os.environ.copy()
    environment.pop("SSH_ASKPASS", None)
    environment["SSH_ASKPASS_REQUIRE"] = "never"
    return subprocess.run(
        [executable, *arguments], input=payload, capture_output=True, env=environment,
        timeout=15, check=False,
    )


def require_signature(data):
    if not data or len(data) > MAX_SIGNATURE_BYTES:
        raise ValueError("Signature is empty or exceeds its byte limit")
    lines = data.splitlines()
    if len(lines) < 3 or lines[0] != SIGNATURE_HEADER or lines[-1] != SIGNATURE_FOOTER:
        raise ValueError("Expected a detached OpenSSH signature")
    return data


def sign(args):
    destination = args.signature.absolute()
    if not stat.S_ISDIR(ordinary_mode(destination.parent)):
        raise ValueError("Signature destination requires an ordinary directory")
    if destination.exists() or destination.is_symlink():
        raise FileExistsError("Signature destination is already occupied")
    key = args.key.absolute()
    if not stat.S_ISREG(ordinary_mode(key)):
        raise ValueError("Signing key must be an explicitly selected ordinary file")
    record, payload = checked_decision(args)
    print(f"Signing {record['status']} for candidate {record['archive_sha256']}", flush=True)
    result = run_ssh(ssh_keygen(args.ssh_keygen), ["-Y", "sign", "-f", str(key), "-n", NAMESPACE], payload)
    if result.returncode != 0:
        raise ValueError("OpenSSH could not sign the reviewed decision")
    _write_new(destination, require_signature(result.stdout), MAX_SIGNATURE_BYTES)
    return record


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    add_inputs(parser)
    parser.add_argument("--key", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        sign(args)
    except (OSError, ValueError, BadZipFile, subprocess.SubprocessError) as error:
        parser.exit(1, f"Release signing failed. {error}\n")
    print(f"Published detached signature at {args.signature}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
