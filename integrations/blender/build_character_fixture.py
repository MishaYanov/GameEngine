"""Generate two revisions of a skinned Blender character and their exported GLBs."""

import argparse
from pathlib import Path
import sys

import bpy
from mathutils import Quaternion

sys.path.insert(0, str(Path(__file__).resolve().parent))
from game_engine_export import register, unregister
from game_engine_export.blender_addon import export_character_selection
from game_engine_export.character_glb import CLIP_ID, JOINT_ID
from game_engine_export.static_glb import MESH_ID, PROVENANCE, read_glb


def create_rig(leg_ik):
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    bpy.context.scene.unit_settings.system = "METRIC"
    bpy.context.scene.unit_settings.scale_length = 1.0

    armature = bpy.data.armatures.new("Fixture skeleton")
    rig = bpy.data.objects.new("Fixture Rig", armature)
    bpy.context.collection.objects.link(rig)
    rig.select_set(True)
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.mode_set(mode="EDIT")
    if leg_ik:
        bones = [
            ("Hip", (0, 0, 2), (0, -0.16, 1)),
            ("Knee", (0, -0.16, 1), (0, 0, 0.2)),
            ("Foot", (0, 0, 0.2), (0, -0.2, 0.1)),
        ]
    else:
        bones = [("Root", (0, 0, 0), (0, 0, 1)), ("Foot", (0, 0, 1), (0, 0, 2))]
    parent = None
    for name, head, tail in bones:
        bone = armature.edit_bones.new(name)
        bone.head = head
        bone.tail = tail
        bone.parent = parent
        parent = bone
    bpy.ops.object.mode_set(mode="OBJECT")

    surface = bpy.data.meshes.new("Fixture surface")
    surface.from_pydata(
        [(-0.3, 0, 0), (0.3, 0, 0), (-0.22, 0, 1), (0.22, 0, 1),
         (-0.12, 0, 2), (0.12, 0.18, 2)],
        [], [(0, 1, 3, 2), (2, 3, 5, 4)],
    )
    surface.update()
    model = bpy.data.objects.new("Fixture Character", surface)
    bpy.context.collection.objects.link(model)
    model.parent = rig
    model.select_set(True)
    model["private-authoring-note"] = "must not leave Blender"
    deform = model.modifiers.new("Fixture skin", "ARMATURE")
    deform.object = rig
    if leg_ik:
        model.vertex_groups.new(name="Hip").add([4, 5], 1.0, "REPLACE")
        model.vertex_groups.new(name="Knee").add([2, 3], 1.0, "REPLACE")
        model.vertex_groups.new(name="Foot").add([0, 1], 1.0, "REPLACE")
    else:
        root_group = model.vertex_groups.new(name="Root")
        root_group.add([0, 1], 1.0, "REPLACE")
        root_group.add([2, 3], 0.5, "REPLACE")
        foot_group = model.vertex_groups.new(name="Foot")
        foot_group.add([2, 3], 0.5, "REPLACE")
        foot_group.add([4, 5], 1.0, "REPLACE")
    return rig


def add_action(rig, name, target, bend):
    animation = rig.animation_data_create()
    animation.action = None
    joint = rig.pose.bones[target]
    joint.rotation_mode = "QUATERNION"
    for frame, angle in [(1, 0.0), (9, bend), (17, 0.0)]:
        joint.rotation_quaternion = Quaternion((1.0, 0.0, 0.0), angle)
        joint.keyframe_insert("rotation_quaternion", frame=frame)
    action = animation.action
    action.name = name
    action["private-authoring-note"] = "do not publish action metadata"
    animation.action = None
    return action


def assign_clips(rig, actions):
    tracks = rig.animation_data.nla_tracks
    for track in list(tracks):
        tracks.remove(track)
    for action in actions:
        track = tracks.new()
        track.name = f"NlaTrack {action.name}"
        track.strips.new(action.name, 1, action)


def exported_identities(path):
    document, _ = read_glb(path)
    mesh, = document["meshes"]
    skin, = document["skins"]
    joints = [document["nodes"][index] for index in skin["joints"]]
    for node in document["nodes"]:
        assert set(node.get("extras", {})) <= {JOINT_ID}, node
    for clip in document["animations"]:
        assert set(clip["extras"]) == {CLIP_ID}, clip
    assert document["asset"]["extras"][PROVENANCE]["profile"] == "character"
    return (
        mesh["extras"][MESH_ID],
        {joint["name"]: joint["extras"][JOINT_ID] for joint in joints},
        {clip["name"]: clip["extras"][CLIP_ID] for clip in document["animations"]},
    )


def export_fixture(directory, leg_ik):
    rig = create_rig(leg_ik)
    idle = add_action(rig, "Idle", "Foot", 0.12)
    walk = add_action(rig, "Walk", "Knee" if leg_ik else "Foot", 0.65)
    assign_clips(rig, [idle, walk])
    rig.data.bones["Foot"]["private-authoring-note"] = "do not publish bone metadata"
    first = directory / "character-fixture.glb"
    revised = directory / "character-fixture-revised.glb"
    missing_clip = directory / "character-fixture-missing-clip.glb"
    source = directory / "character-fixture.blend"

    register()
    try:
        assert bpy.ops.export_scene.game_engine_character_glb(filepath=str(first)) == {"FINISHED"}
        mesh_id, joints, clips = exported_identities(first)
        assert list(clips) == ["Idle", "Walk"], clips

        bpy.context.preferences.filepaths.file_preview_type = "NONE"
        bpy.ops.wm.save_as_mainfile(filepath=str(source), check_existing=False, relative_remap=False)
        bpy.ops.wm.open_mainfile(filepath=str(source), load_ui=False)
        rig = bpy.data.objects["Fixture Rig"]
        idle = bpy.data.actions["Idle"]
        walk = bpy.data.actions["Walk"]
        assert (idle[CLIP_ID], walk[CLIP_ID]) == (clips["Idle"], clips["Walk"])
        rig.data.bones["Foot"].name = "Foot Revised"
        idle.name = "Z Rest"
        walk.name = "A Travel"
        assign_clips(rig, [walk, idle])
        assert bpy.ops.export_scene.game_engine_character_glb(filepath=str(revised)) == {"FINISHED"}

        revised_mesh_id, revised_joints, revised_clips = exported_identities(revised)
        assert revised_mesh_id == mesh_id
        expected_joints = joints.copy()
        expected_joints["Foot Revised"] = expected_joints.pop("Foot")
        assert revised_joints == expected_joints
        assert list(revised_clips) == ["A Travel", "Z Rest"], revised_clips
        assert revised_clips == {"Z Rest": clips["Idle"], "A Travel": clips["Walk"]}

        previous = revised.read_bytes()
        walk[CLIP_ID] = idle[CLIP_ID]
        try:
            export_character_selection(bpy.context, revised)
        except ValueError as error:
            assert "duplicate" in str(error)
        else:
            raise AssertionError("Duplicate clip ID was accepted")
        finally:
            walk[CLIP_ID] = clips["Walk"]
        assert revised.read_bytes() == previous

        assign_clips(rig, [walk])
        assert bpy.ops.export_scene.game_engine_character_glb(filepath=str(missing_clip)) == {"FINISHED"}
        removed_mesh_id, removed_joints, remaining_clips = exported_identities(missing_clip)
        assert (removed_mesh_id, removed_joints) == (mesh_id, revised_joints)
        assert remaining_clips == {"A Travel": clips["Walk"]}
        print("Character GLB fixture passed identity, rename, clip ordering, and output checks")
    finally:
        unregister()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--leg-ik", action="store_true", help="Use a bent three-joint leg for foot IK")
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:])
    directory = args.output.resolve()
    directory.mkdir(parents=True, exist_ok=True)
    export_fixture(directory, args.leg_ik)


if __name__ == "__main__":
    main()
