"""Blender adapter for the Game Engine static GLB exporter."""

import hashlib
import math
from pathlib import Path
from uuid import uuid4

import bpy
from bpy_extras.io_utils import ExportHelper

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


def _validate_material(material):
    if material is None or not material.use_nodes:
        return
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
    if any(socket.is_linked for socket in shader.inputs if socket.name != "Base Color"):
        raise ValueError(f"{material.name}: the profile only supports base-color textures")
    if shader.inputs["Alpha"].default_value != 1.0:
        raise ValueError(f"{material.name}: the static profile requires opaque materials")
    color = shader.inputs["Base Color"]
    if color.is_linked:
        link = color.links[0]
        texture = link.from_node
        if (texture.type != "TEX_IMAGE" or texture.mute or texture.image is None
                or texture.inputs["Vector"].is_linked or link.from_socket.name != "Color"):
            raise ValueError(f"{material.name}: use a direct Image Texture with the active UV map")


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
    for material in materials:
        _validate_material(material)
    return meshes


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


def export_static_selection(context, destination):
    destination = Path(destination)
    parent = destination.parent.resolve(strict=True)
    if any((directory / "asset-catalog.json").exists() for directory in (parent, *parent.parents)):
        raise ValueError("Export to a separate folder, then use Reimport GLB in the Editor")
    meshes = _selected_meshes(context)
    identities = set()
    missing = []
    for mesh in meshes:
        if MESH_ID not in mesh:
            missing.append(mesh)
            continue
        ident = mesh[MESH_ID]
        if not valid_mesh_id(ident) or ident in identities:
            raise ValueError(f"{mesh.name}: fix its invalid or duplicate {MESH_ID} custom property")
        identities.add(ident)

    provenance = _source_provenance(context)
    generated = []
    try:
        for mesh in missing:
            ident = uuid4().hex
            mesh[MESH_ID] = ident
            generated.append(mesh)
            identities.add(ident)

        def export_candidate(path):
            result = bpy.ops.export_scene.gltf(filepath=str(path), **EXPORT_OPTIONS)
            if "FINISHED" not in result:
                raise ValueError("Blender did not finish exporting the selected objects")

        return publish_static_glb(parent / destination.name, export_candidate, identities, provenance)
    except Exception:
        for mesh in generated:
            del mesh[MESH_ID]
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


def _export_menu(self, _context):
    self.layout.operator(GAME_ENGINE_OT_export_static_glb.bl_idname, text="Game Engine (.glb)")


def register():
    bpy.utils.register_class(GAME_ENGINE_OT_export_static_glb)
    bpy.types.TOPBAR_MT_file_export.append(_export_menu)


def unregister():
    bpy.types.TOPBAR_MT_file_export.remove(_export_menu)
    bpy.utils.unregister_class(GAME_ENGINE_OT_export_static_glb)
