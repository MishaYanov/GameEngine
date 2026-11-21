"""Verify a downloaded Windows release ZIP against its trusted receipt."""

import argparse
from hashlib import file_digest
import json
import os
from pathlib import Path
import re
import stat
from zipfile import BadZipFile, ZIP_DEFLATED, ZIP_STORED, ZipFile


MAX_RECEIPT_BYTES = 4 * 1024 * 1024
MAX_PACKAGE_BYTES = 4 * 1024 * 1024 * 1024
MAX_MEMBERS = 8192
SHA256 = re.compile(r"[0-9a-f]{64}")
GIT_REVISION = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})")
WINDOWS_DEVICE = re.compile(r"(?:con|prn|aux|nul|conin\$|conout\$|(?:com|lpt)[1-9¹²³])")
SMOKE_LABELS = ("player-default", "player-arrows")


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("Receipt contains a duplicate field")
        result[key] = value
    return result


def read_receipt(path):
    with path.open("rb") as stream:
        data = stream.read(MAX_RECEIPT_BYTES + 1)
    if len(data) > MAX_RECEIPT_BYTES:
        raise ValueError("Release receipt exceeds its size limit")
    return json.loads(data.decode("utf-8"), object_pairs_hook=_unique_object)


def _is_hash(value):
    return isinstance(value, str) and SHA256.fullmatch(value) is not None


def _path_parts(name):
    if not isinstance(name, str) or len(name) > 1024:
        raise ValueError("Invalid package filename")
    parts = name.split("/")
    for part in parts:
        if (
            not part
            or part.endswith((".", " "))
            or any(c in '\\:<>"|?*' or ord(c) < 32 for c in part)
            or WINDOWS_DEVICE.fullmatch(part.split(".", 1)[0].rstrip().casefold())
        ):
            raise ValueError(f"Unsafe package path {name!r}")
    return parts


def _package_files(receipt):
    version = receipt.get("schema_version") if isinstance(receipt, dict) else None
    if type(version) is not int or version != 1:
        raise ValueError("Unsupported release receipt schema")
    revision = receipt.get("git_revision")
    if not isinstance(revision, str) or not GIT_REVISION.fullmatch(revision):
        raise ValueError("Invalid Git revision in receipt")
    if type(receipt.get("source_dirty")) is not bool or not _is_hash(
        receipt.get("authored_manifest_sha256")
    ):
        raise ValueError("Invalid source evidence in receipt")
    if not _is_hash(receipt.get("archive_sha256")):
        raise ValueError("Invalid archive hash in receipt")
    if receipt.get("smoke") != {label: "passed" for label in SMOKE_LABELS}:
        raise ValueError("Receipt does not record both Player smoke checks")
    expected = receipt.get("package_files")
    if not isinstance(expected, dict) or not 0 < len(expected) <= MAX_MEMBERS:
        raise ValueError("Invalid package inventory in receipt")
    if not {"game.exe", "project.json"} <= expected.keys():
        raise ValueError("Package inventory lacks the Player or project manifest")

    directories = {}
    filenames = set()
    for name, digest in expected.items():
        parts = _path_parts(name)
        if not _is_hash(digest):
            raise ValueError(f"Invalid hash for {name!r}")
        folded_name = name.casefold()
        if folded_name in filenames:
            raise ValueError(f"Duplicate Windows package path {name!r}")
        filenames.add(folded_name)
        for depth in range(1, len(parts)):
            prefix = "/".join(parts[:depth])
            if directories.setdefault(prefix.casefold(), prefix) != prefix:
                raise ValueError(f"Ambiguous Windows package directory {prefix!r}")
    if not filenames.isdisjoint(directories):
        raise ValueError("Package path is both a file and a directory")
    return expected


def verify_candidate(archive, receipt):
    """Check receipt metadata, archive digest, and every regular file in the ZIP."""
    expected = _package_files(receipt)
    with archive.open("rb") as stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size <= MAX_PACKAGE_BYTES:
            raise ValueError("Candidate ZIP is missing or outside the size limit")
        if file_digest(stream, "sha256").hexdigest() != receipt["archive_sha256"]:
            raise ValueError("Unexpected archive hash")
        stream.seek(0)
        with ZipFile(stream) as package:
            members = package.infolist()
            if len(members) != len(expected) or {m.filename for m in members} != expected.keys():
                raise ValueError("Unexpected ZIP inventory")
            total_bytes = 0
            for member in members:
                unix_type = stat.S_IFMT(member.external_attr >> 16)
                dos_type = member.external_attr & (stat.FILE_ATTRIBUTE_REPARSE_POINT | 0x10)
                if (
                    member.orig_filename != member.filename
                    or unix_type not in (0, stat.S_IFREG)
                    or dos_type
                    or member.flag_bits & 1
                    or member.compress_type not in (ZIP_STORED, ZIP_DEFLATED)
                ):
                    raise ValueError(f"Unsupported package member {member.filename!r}")
                total_bytes += member.file_size
                if total_bytes > MAX_PACKAGE_BYTES:
                    raise ValueError("Unpacked ZIP exceeds its size limit")
            for member in members:
                with package.open(member) as payload:
                    actual = file_digest(payload, "sha256").hexdigest()
                if actual != expected[member.filename]:
                    raise ValueError(f"Unexpected member hash for {member.filename!r}")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("receipt", type=Path)
    parser.add_argument("archive", type=Path)
    args = parser.parse_args(argv)
    try:
        verify_candidate(args.archive, read_receipt(args.receipt))
    except (OSError, ValueError, BadZipFile) as error:
        parser.exit(1, f"Candidate verification failed. {error}\n")
    print(f"Verified {args.archive}")


if __name__ == "__main__":
    main()
