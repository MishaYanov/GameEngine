"""Validate and publish the one-skin animated GLB profile."""

from .static_glb import MESH_ID, _index, _list, _publish_glb, _validate_glb_geometry, valid_mesh_id

JOINT_ID = "game_engine_joint_id"
CLIP_ID = "game_engine_clip_id"
PROFILE_VERSION = 1
MAX_JOINTS = 128
MAX_NODES = 256
MAX_CLIPS = 32
CHARACTER_LIMITS = (32 * 1024 * 1024, 2 * 1024 * 1024)


def _clip_identity_records(clips, expected):
    records = []
    seen = set()
    for clip in clips:
        extras = clip.get("extras")
        ident = extras.get(CLIP_ID) if isinstance(extras, dict) else None
        if not valid_mesh_id(ident) or ident in seen:
            raise ValueError(f"Each exported {CLIP_ID} must be valid and unique")
        seen.add(ident)
        records.append((clip, CLIP_ID, ident))
    if seen != set(expected):
        raise ValueError(f"Blender did not export the selected {CLIP_ID} values exactly once")
    return records


def _joint_identity_records(joints, joint_ids):
    # Blender 4.0 exports Bone extras while newer versions export PoseBone
    # extras. The single rig's existing names identify only this export's
    # nodes. The persistent IDs always come from its saved Bone data.
    values = tuple(joint_ids.values())
    if not all(map(valid_mesh_id, values)) or len(values) != len(set(values)):
        raise ValueError(f"Each source {JOINT_ID} must be valid and unique")
    records = []
    names = set()
    for joint in joints:
        name = joint.get("name")
        if not isinstance(name, str) or name not in joint_ids or name in names:
            raise ValueError("Blender did not export the selected joints exactly once")
        names.add(name)
        records.append((joint, JOINT_ID, joint_ids[name]))
    if names != joint_ids.keys():
        raise ValueError("Blender did not export the selected joints exactly once")
    return records


def _validate_scene(document):
    nodes = _list(document, "nodes")
    scenes = _list(document, "scenes")
    if len(scenes) != 1 or document.get("scene", 0) != 0 or len(nodes) > MAX_NODES:
        raise ValueError(f"Character GLB requires one scene with at most {MAX_NODES} nodes")
    roots = scenes[0].get("nodes")
    if not isinstance(roots, list) or not roots or any(not _index(root, nodes) for root in roots):
        raise ValueError("Character scene roots must refer to exported nodes")
    pending = list(roots)
    visited = set()
    while pending:
        index = pending.pop()
        if index in visited:
            raise ValueError("Character scene has a repeated node")
        visited.add(index)
        pending.extend(nodes[index].get("children", []))
    if len(visited) != len(nodes):
        raise ValueError("All character nodes must be reachable from the exported scene")
    return nodes


def _validate_skin(document, nodes):
    skins = _list(document, "skins")
    meshes = _list(document, "meshes")
    render_nodes = [node for node in nodes if "mesh" in node or "skin" in node]
    if len(meshes) != 1 or len(skins) != 1 or len(render_nodes) != 1:
        raise ValueError("Character GLB requires exactly one skinned mesh node and one skin")
    model = render_nodes[0]
    if not _index(model.get("mesh"), meshes) or not _index(model.get("skin"), skins):
        raise ValueError("The character mesh and skin must be on the same scene node")
    joints = skins[0].get("joints")
    if (not isinstance(joints, list) or not 1 <= len(joints) <= MAX_JOINTS
            or any(not _index(index, nodes) for index in joints) or len(set(joints)) != len(joints)):
        raise ValueError(f"Character skin requires 1–{MAX_JOINTS} distinct joints")

    accessors = _list(document, "accessors")
    for primitive in meshes[0]["primitives"]:
        attributes = primitive["attributes"]
        if (any(not _index(attributes.get(key), accessors) for key in ("JOINTS_0", "WEIGHTS_0"))
                or any(key.startswith(("JOINTS_", "WEIGHTS_")) and not key.endswith("_0")
                       for key in attributes)):
            raise ValueError("Character mesh requires one set of skin joints and weights")
    return [nodes[index] for index in joints]


def _validate_clips(document, nodes):
    clips = _list(document, "animations")
    if not 1 <= len(clips) <= MAX_CLIPS:
        raise ValueError(f"Character GLB requires 1–{MAX_CLIPS} animation clips")
    accessors = _list(document, "accessors")
    total_keys = 0
    for clip in clips:
        channels = _list(clip, "channels")
        samplers = _list(clip, "samplers")
        targets = set()
        if not 1 <= len(channels) <= 768:
            raise ValueError("Each character clip requires 1–768 animation channels")
        for channel in channels:
            target = channel.get("target")
            if (not isinstance(target, dict) or not _index(target.get("node"), nodes)
                    or target.get("path") not in ("translation", "rotation", "scale")
                    or not _index(channel.get("sampler"), samplers)):
                raise ValueError("Character clips require a valid transform target and sampler")
            binding = (target["node"], target["path"])
            if binding in targets:
                raise ValueError("Character clip repeats a transform channel")
            targets.add(binding)
            sampler = samplers[channel["sampler"]]
            if (sampler.get("interpolation", "LINEAR") not in ("LINEAR", "STEP")
                    or any(not _index(sampler.get(key), accessors) for key in ("input", "output"))):
                raise ValueError("Character clips require baked linear or step keys")
            count = accessors[sampler["input"]]["count"]
            total_keys += count
            if count > 4096 or accessors[sampler["output"]]["count"] != count or total_keys > 262144:
                raise ValueError("Character clip keys exceed the animation budget or differ from values")
    return clips


def publish_character_glb(destination, export_candidate, mesh_id, joint_ids, clip_ids, provenance):
    """Publish an embedded, bounded character with source-stable identity metadata."""
    def identities(document, binary):
        for field in ("cameras", "extensionsRequired"):
            if document.get(field):
                raise ValueError(f"Character profile does not support glTF {field}")
        _validate_glb_geometry(document, binary, {mesh_id}, skinned=True)
        nodes = _validate_scene(document)
        joints = _validate_skin(document, nodes)
        clips = _validate_clips(document, nodes)
        return [
            (document["meshes"][0], MESH_ID, mesh_id),
            *_joint_identity_records(joints, joint_ids),
            *_clip_identity_records(clips, clip_ids),
        ]

    return _publish_glb(
        destination, export_candidate, identities, {**provenance, "profile": "character"},
        PROFILE_VERSION, limits=CHARACTER_LIMITS,
    )
