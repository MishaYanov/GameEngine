"""Blender adapter for the Game Engine static and character GLB profiles."""

import hashlib
import math
from pathlib import Path
from uuid import uuid4

import bpy
from bpy_extras.io_utils import ExportHelper

from .character_glb import CLIP_ID, JOINT_ID, MAX_CLIPS, MAX_JOINTS, publish_character_glb
from .static_glb import MAX_SCENE_ENTRIES, MESH_ID, publish_static_glb, valid_mesh_id

MAX_SOURCE_BYTES = 512 * 1024 * 1024
EXPORT_OPTIONS = {
    "export_format": "GLB",
    "use_selection": True,
    "use_active_scene": True,
    "export_yup": True,
    "export_extras": True,
    "export_apply": False,
    "export_texcoords": True,
    "export_normals": True,
    "export_materials": "EXPORT",
    "export_image_format": "AUTO",
    "export_animations": False,
    "export_skins": False,
    "export_morph": False,
    "export_cameras": False,
    "export_lights": False,
    "export_draco_mesh_compression_enable": False,
    "export_gpu_instances": False,
}
CHARACTER_OPTIONS = {
    **EXPORT_OPTIONS,
    "export_animations": True,
    "export_animation_mode": "ACTIONS",
    "export_force_sampling": True,
    "export_anim_single_armature": False,
    "export_rest_position_armature": True,
    "export_skins": True,
    "export_influence_nb": 4,
    "export_all_influences": False,
    "export_def_bones": False,
}


def _validate_material(material):
    if material is None or not material.use_nodes:
        return False
    nodes = material.node_tree.nodes
    output = next(
        (node for node in nodes if node.type == "OUTPUT_MATERIAL" and node.is_active_output),
        None,
    )
    if output is None:
        raise ValueError(f"{material.name}: material needs an active output")
    surface = output.inputs["Surface"]
    if len(surface.links) != 1 or any(
        socket.is_linked for socket in output.inputs if socket.name != "Surface"
    ):
        raise ValueError(f"{material.name}: only a connected surface material is supported")
    shader = surface.links[0].from_node
    if shader.type != "BSDF_PRINCIPLED" or shader.mute:
        raise ValueError(f"{material.name}: connect a Principled BSDF to Surface")
    if any(socket.is_linked for socket in shader.inputs if socket.name not in {"Base Color", "Normal"}):
        raise ValueError(f"{material.name}: the profile only supports base color and tangent-space normals")
    if shader.inputs["Alpha"].default_value != 1.0:
        raise ValueError(f"{material.name}: the export profile requires opaque materials")
    color = shader.inputs["Base Color"]
    if color.is_linked:
        link = color.links[0]
        texture = link.from_node
        if (texture.type != "TEX_IMAGE" or texture.mute or texture.image is None
                or texture.inputs["Vector"].is_linked or link.from_socket.name != "Color"):
            raise ValueError(f"{material.name}: use a direct Image Texture with the active UV map")
    return _validate_normal_map(material, shader.inputs["Normal"])


def _validate_normal_map(material, socket):
    if not socket.is_linked:
        return False
    link = socket.links[0]
    normal = link.from_node
    if (normal.type != "NORMAL_MAP" or normal.mute or normal.space != "TANGENT"
            or normal.uv_map or link.from_socket.name != "Normal"):
        raise ValueError(f"{material.name}: connect a tangent-space Normal Map using the active UV map")

    strength = normal.inputs["Strength"]
    if strength.is_linked or not math.isfinite(strength.default_value) or strength.default_value < 0:
        raise ValueError(f"{material.name}: set a finite nonnegative Normal Map strength")

    color = normal.inputs["Color"]
    if not color.is_linked:
        raise ValueError(f"{material.name}: connect an Image Texture to the Normal Map Color input")
    image_link = color.links[0]
    texture = image_link.from_node
    if (texture.type != "TEX_IMAGE" or texture.mute or texture.image is None
            or texture.inputs["Vector"].is_linked or image_link.from_socket.name != "Color"):
        raise ValueError(f"{material.name}: connect a direct Image Texture using the active UV map")
    if texture.image.colorspace_settings.name != "Non-Color":
        raise ValueError(f"{material.name}: set the normal Image Texture color space to Non-Color")
    if texture.image.source not in {"FILE", "GENERATED"}:
        raise ValueError(f"{material.name}: use one normal image instead of a sequence or tiled image")
    return True


def _selected_meshes(context):
    if context.mode != "OBJECT":
        raise ValueError("Switch to Object Mode before exporting")
    if not math.isclose(context.scene.unit_settings.scale_length, 1.0, rel_tol=1e-6):
        raise ValueError("Set the scene Unit Scale to 1 for the static export profile")
    selected = set(context.selected_objects)
    if len(selected) > MAX_SCENE_ENTRIES:
        raise ValueError(f"Select at most {MAX_SCENE_ENTRIES} objects per export")
    meshes = set()
    materials = set()
    for obj in selected:
        if obj.type not in {"MESH", "EMPTY"} or obj.instance_type != "NONE":
            raise ValueError(f"{obj.name}: choose static meshes and ordinary parent empties")
        if obj.parent is not None and obj.parent not in selected:
            raise ValueError(f"Include {obj.parent.name}, the parent of {obj.name}")
        animation = obj.animation_data
        if obj.constraints or (animation and (animation.action or animation.drivers or animation.nla_tracks)):
            raise ValueError(f"{obj.name}: apply constraints and remove animation before exporting")
        if obj.type == "MESH":
            mesh = obj.data
            if obj.modifiers or mesh.shape_keys or mesh.animation_data:
                raise ValueError(f"{obj.name}: use a static mesh with applied modifiers and no shape keys")
            if mesh.library or mesh.override_library:
                raise ValueError(f"{obj.name}: make linked mesh data local before exporting")
            if any(slot.link != "DATA" for slot in obj.material_slots):
                raise ValueError(f"{obj.name}: store materials on the mesh data before exporting")
            meshes.add(mesh)
            materials.update(mesh.materials)
    if not meshes:
        raise ValueError("Select at least one mesh and the parents you want to export")
    return meshes, _validate_materials(materials)


def _validate_materials(materials):
    has_normal_map = False
    for material in materials:
        has_normal_map |= _validate_material(material)
    return has_normal_map


def _source_provenance(context):
    provenance = {
        "blender_version": bpy.app.version_string,
        "scene": context.scene.name,
        "had_unsaved_changes": bool(bpy.data.is_dirty),
    }
    if bpy.data.filepath:
        source = Path(bpy.data.filepath)
        provenance["source_name"] = source.name
        if source.is_file() and source.stat().st_size <= MAX_SOURCE_BYTES:
            with source.open("rb") as stream:
                digest = hashlib.sha256()
                remaining = MAX_SOURCE_BYTES
                while block := stream.read(min(1 << 20, remaining + 1)):
                    remaining -= len(block)
                    if remaining < 0:
                        break
                    digest.update(block)
                else:
                    provenance["saved_source_sha256"] = digest.hexdigest()
    return provenance


def _safe_destination(destination):
    destination = Path(destination)
    parent = destination.parent.resolve(strict=True)
    if any((directory / "asset-catalog.json").exists() for directory in (parent, *parent.parents)):
        raise ValueError("Export to a separate folder, then use Reimport GLB in the Editor")
    return parent / destination.name


def _source_ids(blocks, key, generated):
    identities = set()
    missing = []
    for block in blocks:
        if key not in block:
            missing.append(block)
            continue
        ident = block[key]
        if not valid_mesh_id(ident) or ident in identities:
            raise ValueError(f"{block.name}: fix its invalid or duplicate {key} custom property")
        identities.add(ident)
    for block in missing:
        ident = uuid4().hex
        block[key] = ident
        generated.append((block, key))
        identities.add(ident)
    return identities


def _export_candidate(path, options):
    result = bpy.ops.export_scene.gltf(filepath=str(path), **options)
    if "FINISHED" not in result:
        raise ValueError("Blender did not finish exporting the selected objects")


def export_static_selection(context, destination):
    destination = _safe_destination(destination)
    meshes, tangents = _selected_meshes(context)
    options = {**EXPORT_OPTIONS, "export_tangents": tangents}
    provenance = _source_provenance(context)
    generated = []
    try:
        identities = _source_ids(meshes, MESH_ID, generated)
        return publish_static_glb(
            destination, lambda path: _export_candidate(path, options), identities, provenance,
        )
    except Exception:
        for block, key in generated:
            del block[key]
        raise


def _character_actions(armature):
    animation = armature.animation_data
    if animation is None or animation.use_tweak_mode:
        raise ValueError("Assign actions to the armature and exit NLA Tweak Mode before exporting")
    actions = [animation.action] if animation.action else []
    for track in animation.nla_tracks:
        strips = [strip for strip in track.strips if strip.action is not None and not strip.mute]
        if len(strips) > 1:
            raise ValueError(f"{track.name}: use one action strip per NLA track")
        actions.extend(strip.action for strip in strips)
    if not 1 <= len(actions) <= MAX_CLIPS:
        raise ValueError(f"Assign 1–{MAX_CLIPS} actions to the armature, using single-strip NLA tracks")
    if len(set(actions)) != len(actions):
        raise ValueError("Assign each action only once, as the active action or on an NLA track")
    if any(action.library or action.override_library for action in actions):
        raise ValueError("Make the character's actions local before exporting")
    return actions


def _selected_character(context):
    if context.mode != "OBJECT":
        raise ValueError("Switch to Object Mode before exporting the character")
    if not math.isclose(context.scene.unit_settings.scale_length, 1.0, rel_tol=1e-6):
        raise ValueError("Set the scene Unit Scale to 1 for the character export profile")
    selected = list(context.selected_objects)
    if len(selected) != 2 or {obj.type for obj in selected} != {"ARMATURE", "MESH"}:
        raise ValueError("Select one armature and its skinned mesh")
    armature = next(obj for obj in selected if obj.type == "ARMATURE")
    model = next(obj for obj in selected if obj.type == "MESH")
    if (any(obj.instance_type != "NONE" for obj in selected) or armature.parent is not None
            or model.parent not in (None, armature)):
        raise ValueError("The character armature must be a root, with only its selected associated mesh")
    mods = list(model.modifiers)
    if (len(mods) != 1 or mods[0].type != "ARMATURE" or mods[0].object != armature
            or not mods[0].show_viewport or not mods[0].use_vertex_groups):
        raise ValueError("Use one enabled Armature modifier with vertex groups on the selected mesh")
    if model.data.shape_keys or model.animation_data or model.data.animation_data:
        raise ValueError("Character export uses armature actions and a mesh without shape keys or animation")
    if any(obj.data.library or obj.data.override_library for obj in selected):
        raise ValueError("Make the character's mesh and armature data local before exporting")
    if any(slot.link != "DATA" for slot in model.material_slots):
        raise ValueError("Store character materials on the mesh data before exporting")
    tangents = _validate_materials(set(model.data.materials))
    bones = list(armature.data.bones)
    if not 1 <= len(bones) <= MAX_JOINTS:
        raise ValueError(f"Character export requires 1–{MAX_JOINTS} bones")
    return model.data, bones, _character_actions(armature), tangents


def export_character_selection(context, destination):
    destination = _safe_destination(destination)
    mesh, bones, actions, tangents = _selected_character(context)
    options = {**CHARACTER_OPTIONS, "export_tangents": tangents}
    provenance = _source_provenance(context)
    generated = []
    try:
        mesh_id, = _source_ids([mesh], MESH_ID, generated)
        _source_ids(bones, JOINT_ID, generated)
        joint_ids = {bone.name: bone[JOINT_ID] for bone in bones}
        clip_ids = _source_ids(actions, CLIP_ID, generated)
        return publish_character_glb(
            destination, lambda path: _export_candidate(path, options),
            mesh_id, joint_ids, clip_ids, provenance,
        )
    except Exception:
        for block, key in generated:
            del block[key]
        raise


class GAME_ENGINE_OT_export_static_glb(bpy.types.Operator, ExportHelper):
    """Export selected static meshes and their hierarchy for the Game Engine Editor."""

    bl_idname = "export_scene.game_engine_glb"
    bl_label = "Export Game Engine GLB"
    filename_ext = ".glb"
    filter_glob: bpy.props.StringProperty(default="*.glb", options={"HIDDEN"})

    def execute(self, context):
        try:
            export_static_selection(context, self.filepath)
        except (OSError, ValueError, RuntimeError, TypeError) as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        self.report({"INFO"}, "Exported GLB. Save the blend file to retain new mesh IDs.")
        return {"FINISHED"}


class GAME_ENGINE_OT_export_character_glb(bpy.types.Operator, ExportHelper):
    """Export one armature, its skinned mesh, and its actions for the Game Engine Editor."""

    bl_idname = "export_scene.game_engine_character_glb"
    bl_label = "Export Game Engine Character GLB"
    filename_ext = ".glb"
    filter_glob: bpy.props.StringProperty(default="*.glb", options={"HIDDEN"})

    def execute(self, context):
        try:
            export_character_selection(context, self.filepath)
        except (OSError, ValueError, RuntimeError, TypeError) as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        self.report({"INFO"}, "Exported Character GLB. Save the blend file to retain new identities.")
        return {"FINISHED"}


def _export_menu(self, _context):
    self.layout.operator(GAME_ENGINE_OT_export_static_glb.bl_idname, text="Game Engine (.glb)")
    self.layout.operator(GAME_ENGINE_OT_export_character_glb.bl_idname, text="Game Engine Character (.glb)")


def register():
    bpy.utils.register_class(GAME_ENGINE_OT_export_static_glb)
    bpy.utils.register_class(GAME_ENGINE_OT_export_character_glb)
    bpy.types.TOPBAR_MT_file_export.append(_export_menu)


def unregister():
    bpy.types.TOPBAR_MT_file_export.remove(_export_menu)
    bpy.utils.unregister_class(GAME_ENGINE_OT_export_character_glb)
    bpy.utils.unregister_class(GAME_ENGINE_OT_export_static_glb)
