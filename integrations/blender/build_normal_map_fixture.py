"""Generate static and skinned Blender GLBs with a normal-texture-only revision."""

import argparse
from pathlib import Path
import sys

import bpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from build_character_fixture import add_action, assign_clips, create_rig, exported_identities
from build_static_fixture import mesh_object
from game_engine_export.blender_addon import export_character_selection, export_static_selection
from game_engine_export.static_glb import MESH_ID, read_glb


def normal_image(name, red):
    image = bpy.data.images.new(name, width=2, height=2)
    image.colorspace_settings.name = "Non-Color"
    image.pixels[:] = (red, 0.5, 0.9, 1.0) * 4
    image.pack()
    return image


def mapped_texture(model):
    material = bpy.data.materials.new("Fixture tangent-space normal")
    material.use_nodes = True
    nodes = material.node_tree.nodes
    links = material.node_tree.links
    texture = nodes.new("ShaderNodeTexImage")
    texture.image = normal_image("Fixture normal +X", 0.75)
    normal = nodes.new("ShaderNodeNormalMap")
    normal.inputs["Strength"].default_value = 0.625
    links.new(texture.outputs["Color"], normal.inputs["Color"])
    links.new(normal.outputs["Normal"], nodes.get("Principled BSDF").inputs["Normal"])
    model.data.materials.append(material)
    return texture


def inspect(path):
    document, binary = read_glb(path)
    mesh, = document["meshes"]
    primitive, = mesh["primitives"]
    material = document["materials"][primitive["material"]]
    normal = material["normalTexture"]
    assert normal.get("texCoord", 0) == 0 and normal["scale"] == 0.625, normal
    image = document["images"][document["textures"][normal["index"]]["source"]]
    assert image["mimeType"] == "image/png", image
    uv = document["accessors"][primitive["attributes"]["TEXCOORD_0"]]
    tangent = document["accessors"][primitive["attributes"]["TANGENT"]]
    assert uv["count"] == tangent["count"] and uv["type"] == "VEC2" and tangent["type"] == "VEC4"

    def data(index):
        view = document["bufferViews"][index]
        offset = view.get("byteOffset", 0)
        return bytes(binary[offset:offset + view["byteLength"]])

    return mesh["extras"][MESH_ID], primitive["material"], data(uv["bufferView"]), data(
        tangent["bufferView"],
    ), data(image["bufferView"])


def revise(directory, name, model, export):
    texture = mapped_texture(model)
    first = directory / f"{name}.glb"
    second = directory / f"{name}-revised.glb"
    export(bpy.context, first)
    texture.image = normal_image("Fixture normal -X", 0.25)
    export(bpy.context, second)
    before, after = inspect(first), inspect(second)
    assert before[:-1] == after[:-1], "Normal-only revision changed mesh identity, material or UV basis"
    assert before[-1] != after[-1], "Normal image bytes did not change"

    previous = second.read_bytes()
    texture.image.colorspace_settings.name = "sRGB"
    try:
        export(bpy.context, second)
    except ValueError as error:
        assert "Non-Color" in str(error), error
    else:
        raise AssertionError("Normal textures in the wrong color space were accepted")
    assert second.read_bytes() == previous
    print(f"{name}: verified UV, tangents, scale, PNG, identity, and texture-only revision")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:])
    directory = args.output.resolve()
    directory.mkdir(parents=True, exist_ok=True)

    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    surface = mesh_object("Fixture normal surface", [(0, 0, 0), (2, 0, 0), (0, 1, 0)], None)
    revise(directory, "static-normal", surface, export_static_selection)

    rig = create_rig(False)
    assign_clips(rig, [add_action(rig, "Idle", "Foot", 0.12), add_action(rig, "Walk", "Foot", 0.65)])
    character = bpy.data.objects["Fixture Character"]
    mesh = character.data
    uv = mesh.uv_layers.new(name="Fixture UV")
    for loop, value in zip(mesh.loops, uv.data, strict=True):
        index = loop.vertex_index
        value.uv = (index % 2, (index // 2) * 0.5)
    revise(directory, "character-normal", character, export_character_selection)
    assert exported_identities(directory / "character-normal.glb") == exported_identities(
        directory / "character-normal-revised.glb",
    )


if __name__ == "__main__":
    main()
