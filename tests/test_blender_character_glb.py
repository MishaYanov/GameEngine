"""Test character identity injection and failed export publication without Blender."""

import copy
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from integrations.blender.game_engine_export import character_glb, static_glb

BINARY = bytes(256)
MESH_ID = static_glb.MESH_ID
JOINT_ID = character_glb.JOINT_ID
CLIP_ID = character_glb.CLIP_ID
JOINT_IDS = {"Root": "joint-root", "Foot": "joint-foot"}


def clip(name, ident):
    return {
        "name": name,
        "extras": {CLIP_ID: ident, "private": "notes"},
        "channels": [{"sampler": 0, "target": {"node": 2, "path": "rotation"}}],
        "samplers": [{"input": 4, "output": 5}],
    }


DOCUMENT = {
    "asset": {"version": "2.0"},
    "buffers": [{"byteLength": len(BINARY)}],
    "bufferViews": [{"buffer": 0, "byteLength": len(BINARY)}],
    "accessors": [
        {"bufferView": 0, "count": count, "componentType": 5126, "type": kind}
        for count, kind in [(3, "VEC3"), (3, "VEC3"), (3, "VEC4"), (3, "VEC4"), (2, "SCALAR"), (2, "VEC4")]
    ],
    "meshes": [{
        "extras": {MESH_ID: "character-mesh", "private": "mesh notes"},
        "primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1, "JOINTS_0": 2, "WEIGHTS_0": 3}}],
    }],
    "nodes": [
        {"mesh": 0, "skin": 0, "extras": {"private": "model note"}},
        {"name": "Root", "children": [2]},
        {"name": "Foot", "extras": {JOINT_ID: "unrelated-pose-data"}},
    ],
    "skins": [{"joints": [1, 2]}],
    "scenes": [{"nodes": [0, 1]}],
    "animations": [clip("Idle", "clip-idle"), clip("Walk", "clip-walk")],
}


def publish(output, document, joint_ids=JOINT_IDS):
    def export_candidate(path):
        with path.open("wb") as stream:
            static_glb._write_glb(stream, document, BINARY)

    character_glb.publish_character_glb(
        output, export_candidate, "character-mesh", joint_ids, {"clip-idle", "clip-walk"}, {},
    )


class CharacterGlbTests(unittest.TestCase):
    def test_publishes_only_stable_ids_after_rename_and_reorder(self):
        with TemporaryDirectory() as root:
            document = copy.deepcopy(DOCUMENT)
            model, parent, foot = document["nodes"]
            foot["name"] = "Renamed Foot"
            parent["children"] = [1]
            document["nodes"] = [model, foot, parent]
            document["scenes"][0]["nodes"] = [0, 2]
            document["skins"][0]["joints"] = [2, 1]
            document["animations"] = [clip("Run", "clip-walk"), clip("Rest", "clip-idle")]
            for animation in document["animations"]:
                animation["channels"][0]["target"]["node"] = 1
            output = Path(root) / "character.glb"
            publish(output, document, {"Root": "joint-root", "Renamed Foot": "joint-foot"})
            actual, _ = static_glb.read_glb(output)
            self.assertEqual(actual["meshes"][0]["extras"], {MESH_ID: "character-mesh"})
            self.assertEqual(actual["nodes"][1]["extras"], {JOINT_ID: "joint-foot"})
            self.assertEqual(actual["nodes"][2]["extras"], {JOINT_ID: "joint-root"})
            self.assertNotIn("extras", actual["nodes"][0])
            self.assertEqual(
                [anim["extras"] for anim in actual["animations"]],
                [{CLIP_ID: "clip-walk"}, {CLIP_ID: "clip-idle"}],
            )

    def test_invalid_rig_and_clips_leave_the_previous_export_intact(self):
        def use_cubic_interpolation(document):
            document["animations"][0]["samplers"][0]["interpolation"] = "CUBICSPLINE"

        edits = [
            ("selected joints", lambda doc: doc["nodes"][2].update(name="Unknown")),
            ("unique", lambda doc: doc["animations"][1]["extras"].update({CLIP_ID: "clip-idle"})),
            ("linear or step", use_cubic_interpolation),
            ("skinned mesh node", lambda doc: doc["nodes"][1].update(mesh=0, skin=0)),
        ]
        with TemporaryDirectory() as root:
            output = Path(root) / "character.glb"
            output.write_bytes(b"previous export")
            for expected, edit in edits:
                with self.subTest(expected=expected):
                    document = copy.deepcopy(DOCUMENT)
                    edit(document)
                    with self.assertRaisesRegex(ValueError, expected):
                        publish(output, document)
                    self.assertEqual(output.read_bytes(), b"previous export")
            self.assertEqual(list(Path(root).iterdir()), [output])
