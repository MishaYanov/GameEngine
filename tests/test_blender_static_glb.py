"""Static GLB profile and publication behavior without a Blender installation."""

import copy
import hashlib
from pathlib import Path
import struct
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from integrations.blender.game_engine_export import static_glb

BINARY = struct.pack("<18f", 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 1)
DOCUMENT = {
    "asset": {"version": "2.0", "generator": "test exporter"},
    "buffers": [{"byteLength": len(BINARY)}],
    "bufferViews": [
        {"buffer": 0, "byteLength": 36},
        {"buffer": 0, "byteOffset": 36, "byteLength": 36},
    ],
    "accessors": [
        {"bufferView": view, "componentType": 5126, "count": 3, "type": "VEC3"}
        for view in (0, 1)
    ],
    "meshes": [{
        "extras": {"game_engine_mesh_id": "asymmetric-surface", "secret": "private authoring data"},
        "primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}}],
    }],
    "nodes": [{"mesh": 0, "extras": {"author": "private note"}}],
    "scenes": [{"nodes": [0]}],
}
EXPECTED_IDS = {"asymmetric-surface"}


def candidate(path, document):
    with path.open("wb") as output:
        static_glb._write_glb(output, document, BINARY)


class StaticGlbTests(unittest.TestCase):
    def test_publishes_identity_and_provenance_without_private_custom_properties(self):
        with TemporaryDirectory() as root:
            output = Path(root) / "surface.glb"
            digest = static_glb.publish_static_glb(
                output, lambda path: candidate(path, DOCUMENT), EXPECTED_IDS, {"blender_version": "fixture"},
            )
            document, binary = static_glb.read_glb(output)
            self.assertEqual(static_glb.validate_static_glb(document, binary, EXPECTED_IDS), EXPECTED_IDS)
            self.assertEqual(document["meshes"][0]["extras"], {"game_engine_mesh_id": "asymmetric-surface"})
            self.assertNotIn("extras", document["nodes"][0])
            provenance = document["asset"]["extras"]["game_engine_export"]
            self.assertEqual(provenance["profile_version"], 1)
            self.assertEqual(provenance["content_sha256"], digest)
            self.assertEqual(provenance["embedded_data_sha256"], hashlib.sha256(BINARY).hexdigest())

    def test_rejects_duplicate_missing_and_malformed_mesh_keys(self):
        for key, extra_mesh in [(None, False), ("bad/key", False), ("asymmetric-surface", True)]:
            with self.subTest(key=key, extra_mesh=extra_mesh):
                document = copy.deepcopy(DOCUMENT)
                document["meshes"][0]["extras"]["game_engine_mesh_id"] = key
                if extra_mesh:
                    document["meshes"].append(copy.deepcopy(document["meshes"][0]))
                with self.assertRaisesRegex(ValueError, "unique Game Engine mesh ID"):
                    static_glb.validate_static_glb(document, BINARY, EXPECTED_IDS)
        with self.assertRaisesRegex(ValueError, "selected mesh IDs"):
            static_glb.validate_static_glb(DOCUMENT, BINARY, {"different-selected-model"})

    def test_rejects_external_images_extensions_animation_and_nontriangle_geometry(self):
        edits = [
            ("embedded PNG or JPEG", lambda doc: doc.update(images=[{"uri": "outside.png"}])),
            ("extensionsUsed", lambda doc: doc.update(extensionsUsed=["KHR_lights_punctual"])),
            ("animations", lambda doc: doc.update(animations=[{}])),
            ("triangles", lambda doc: doc["meshes"][0]["primitives"][0].update(mode=1)),
        ]
        for expected, edit in edits:
            with self.subTest(expected=expected):
                document = copy.deepcopy(DOCUMENT)
                edit(document)
                with self.assertRaisesRegex(ValueError, expected):
                    static_glb.validate_static_glb(document, BINARY, EXPECTED_IDS)

    def test_invalid_or_unpublishable_export_preserves_existing_destination(self):
        with TemporaryDirectory() as root:
            directory = Path(root)
            output = directory / "surface.glb"
            output.write_bytes(b"previous export")
            with self.assertRaisesRegex(ValueError, "complete GLB"):
                static_glb.publish_static_glb(
                    output, lambda path: path.write_bytes(b"incomplete"), EXPECTED_IDS, {},
                )
            with patch.object(static_glb.os, "replace", side_effect=PermissionError("destination is locked")):
                with self.assertRaisesRegex(PermissionError, "destination is locked"):
                    static_glb.publish_static_glb(
                        output, lambda path: candidate(path, DOCUMENT), EXPECTED_IDS, {},
                    )
            self.assertEqual(output.read_bytes(), b"previous export")
            self.assertEqual(list(directory.iterdir()), [output])

    def test_reader_has_a_hard_file_budget(self):
        with TemporaryDirectory() as root:
            output = Path(root) / "surface.glb"
            candidate(output, DOCUMENT)
            with patch.object(static_glb, "MAX_GLB_BYTES", 32):
                with self.assertRaisesRegex(ValueError, "export limit"):
                    static_glb.read_glb(output)
