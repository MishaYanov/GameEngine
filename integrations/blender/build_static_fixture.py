"""Generate repeatable source and export revisions with Blender in background mode."""

import argparse
from pathlib import Path
import sys

import bpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from game_engine_export import register, unregister
from game_engine_export.blender_addon import export_static_selection
from game_engine_export.static_glb import MESH_ID, PROVENANCE, read_glb


def mesh_object(name, vertices, parent):
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(vertices, [], [(0, 1, 2)])
    mesh.update()
    uv = mesh.uv_layers.new(name="Fixture UV")
    for loop, coordinate in zip(uv.data, [(0, 0), (1, 0), (0.25, 1)], strict=True):
        loop.uv = coordinate
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    obj.parent = parent
    obj.select_set(True)
    return obj


def add_base_color_texture(body):
    material = bpy.data.materials.new("Embedded checker")
    material.use_nodes = True
    image = bpy.data.images.new("Fixture checker", width=2, height=2)
    image.pixels = (1, 0, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0.25, 1)
    image.pack()
    texture = material.node_tree.nodes.new("ShaderNodeTexImage")
    texture.image = image
    shader = material.node_tree.nodes.get("Principled BSDF")
    material.node_tree.links.new(texture.outputs["Color"], shader.inputs["Base Color"])
    body.data.materials.append(material)


def export_fixture(directory):
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    bpy.context.scene.unit_settings.system = "METRIC"
    bpy.context.scene.unit_settings.scale_length = 1.0
    parent = bpy.data.objects.new("FixtureRoot", None)
    bpy.context.collection.objects.link(parent)
    parent.location = (3.5, 5.5, 7.5)
    parent.select_set(True)
    body = mesh_object("FixtureBody", [(0, 0, 0), (2, 0, 0.25), (0, 1, 2)], parent)
    body.data[MESH_ID] = "fixture-body"
    add_base_color_texture(body)
    cap = mesh_object("FixtureCap", [(0, 0, 0), (0.5, 0, 0), (0, 0.3, 1)], parent)
    cap.location = (1, -2, 3)
    cap.scale = (-1.25, 0.75, 1.5)
    cap["private-authoring-note"] = "must not leave Blender"
    bpy.context.view_layer.objects.active = body

    first = directory / "static-fixture.glb"
    second = directory / "static-fixture-revised.glb"
    source = directory / "static-fixture.blend"
    register()
    try:
        assert bpy.ops.export_scene.game_engine_glb(filepath=str(first)) == {"FINISHED"}
        cap_id = cap.data[MESH_ID]
        bpy.ops.wm.save_as_mainfile(filepath=str(source), check_existing=False)
        bpy.ops.wm.open_mainfile(filepath=str(source), load_ui=False)
        body = bpy.data.objects["FixtureBody"]
        cap = bpy.data.objects["FixtureCap"]
        assert cap.data[MESH_ID] == cap_id
        body.data.vertices[1].co.x = 2.75
        body.data.name = "FixtureBody Revised"
        bpy.ops.wm.save_as_mainfile(filepath=str(source), check_existing=False)
        assert bpy.ops.export_scene.game_engine_glb(filepath=str(second)) == {"FINISHED"}

        for index, file in enumerate((first, second)):
            doc, _ = read_glb(file)
            root = next(node for node in doc["nodes"] if node["name"] == "FixtureRoot")
            cap_node = next(node for node in doc["nodes"] if node["name"] == "FixtureCap")
            assert root["translation"] == [3.5, 7.5, -5.5], root
            assert cap_node["translation"] == [1.0, 3.0, 2.0], cap_node
            assert cap_node["scale"][0] * cap_node["scale"][1] * cap_node["scale"][2] < 0
            assert len(root["children"]) == 2
            assert "extras" not in cap_node
            assert doc["images"][0]["mimeType"] == "image/png"
            assert {mesh["extras"][MESH_ID] for mesh in doc["meshes"]} == {"fixture-body", cap_id}
            provenance = doc["asset"]["extras"][PROVENANCE]
            assert provenance["profile_version"] == 1
            if index:
                assert provenance["source_name"] == source.name
                assert len(provenance["saved_source_sha256"]) == 64

        original = second.read_bytes()
        cap.data[MESH_ID] = "fixture-body"
        try:
            export_static_selection(bpy.context, second)
        except ValueError as error:
            assert "duplicate" in str(error)
        else:
            raise AssertionError("Duplicate mesh ID was accepted")
        finally:
            cap.data[MESH_ID] = cap_id
        assert second.read_bytes() == original
        print("Static GLB fixture passed Blender axis, hierarchy, texture, identity, and output checks")
    finally:
        unregister()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:])
    directory = args.output.resolve()
    directory.mkdir(parents=True, exist_ok=True)
    export_fixture(directory)


if __name__ == "__main__":
    main()
