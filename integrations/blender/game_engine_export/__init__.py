"""Installable Blender add-on for Game Engine model exports."""

bl_info = {
    "name": "Game Engine Static GLB",
    "author": "Game Engine",
    "version": (1, 1, 0),
    "blender": (4, 0, 0),
    "location": "File > Export > Game Engine (.glb)",
    "description": "Export static models or skinned characters for Game Engine import and reimport",
    "category": "Import-Export",
}


def register():
    from . import blender_addon

    blender_addon.register()


def unregister():
    from . import blender_addon

    blender_addon.unregister()
