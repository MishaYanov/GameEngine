"""Installable Blender add-on for the Game Engine static model profile."""

bl_info = {
    "name": "Game Engine Static GLB",
    "author": "Game Engine",
    "version": (1, 0, 0),
    "blender": (4, 0, 0),
    "location": "File > Export > Game Engine (.glb)",
    "description": "Export a selected static hierarchy for Game Engine import or reimport",
    "category": "Import-Export",
}


def register():
    from . import blender_addon

    blender_addon.register()


def unregister():
    from . import blender_addon

    blender_addon.unregister()
