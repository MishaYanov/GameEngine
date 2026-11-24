"""Validate the static GLB profile, constrain metadata, and publish a single file."""

import hashlib
import json
import math
import os
from pathlib import Path
import re
import struct
from tempfile import TemporaryDirectory

PROFILE_VERSION = 1
MESH_ID = "game_engine_mesh_id"
PROVENANCE = "game_engine_export"
MAX_GLB_BYTES = 64 * 1024 * 1024
MAX_SCENE_ENTRIES = 10_000
MAX_JSON_BYTES = 8 * 1024 * 1024
MESH_ID_PATTERN = re.compile(r"[a-zA-Z0-9_.-]{1,128}", re.ASCII)
HEADER = struct.Struct("<4sII")
CHUNK_HEADER = struct.Struct("<I4s")


def valid_mesh_id(value):
    return isinstance(value, str) and MESH_ID_PATTERN.fullmatch(value) is not None


def _invalid_number(value):
    raise ValueError(f"Invalid glTF number {value}")


def _read_chunk(data, offset, chunk_type):
    if offset + CHUNK_HEADER.size > len(data):
        raise ValueError("GLB chunk header is truncated")
    size, kind = CHUNK_HEADER.unpack_from(data, offset)
    start = offset + CHUNK_HEADER.size
    end = start + size
    if kind != chunk_type or not size or size % 4 or end > len(data):
        raise ValueError(f"GLB has an invalid {chunk_type!r} chunk")
    return data[start:end], end


def read_glb(path):
    """Read a bounded GLB 2 file with one JSON and one BIN chunk."""
    with Path(path).open("rb") as source:
        raw = source.read(MAX_GLB_BYTES + 1)
    if len(raw) > MAX_GLB_BYTES:
        raise ValueError("GLB exceeds the 64 MiB export limit")
    if len(raw) < HEADER.size or HEADER.unpack_from(raw) != (b"glTF", 2, len(raw)):
        raise ValueError("Expected a complete GLB 2 file")
    data = memoryview(raw)
    json_chunk, offset = _read_chunk(data, HEADER.size, b"JSON")
    if len(json_chunk) > MAX_JSON_BYTES:
        raise ValueError("GLB JSON exceeds the 8 MiB export limit")
    binary, end = _read_chunk(data, offset, b"BIN\0")
    if end != len(data):
        raise ValueError("GLB contains trailing chunks")
    try:
        document = json.loads(bytes(json_chunk).decode("utf-8"), parse_constant=_invalid_number)
    except (UnicodeError, RecursionError) as error:
        raise ValueError("GLB contains invalid JSON") from error
    if not isinstance(document, dict):
        raise ValueError("GLB JSON must be an object")
    return document, binary


def _index(value, entries):
    return type(value) is int and 0 <= value < len(entries)


def _list(document, field):
    value = document.get(field, [])
    if (not isinstance(value, list) or len(value) > MAX_SCENE_ENTRIES
            or any(not isinstance(entry, dict) for entry in value)):
        raise ValueError(f"glTF {field} must contain at most {MAX_SCENE_ENTRIES} objects")
    return value


def _vector(value, size):
    return isinstance(value, list) and len(value) == size and all(
        type(x) in (int, float) and -3.402823466e38 <= x <= 3.402823466e38 and math.isfinite(x)
        for x in value
    )


def validate_static_glb(document, binary, expected_ids):
    """Check the export profile. The native importer still validates geometry and images."""
    asset = document.get("asset")
    if not isinstance(asset, dict) or asset.get("version") != "2.0":
        raise ValueError("Expected glTF asset version 2.0")
    for option in ("animations", "skins", "cameras", "extensionsRequired", "extensionsUsed"):
        if document.get(option):
            raise ValueError(f"Static profile does not support glTF {option}")
    buffers = _list(document, "buffers")
    if len(buffers) != 1 or "uri" in buffers[0]:
        raise ValueError("Static GLB requires a single embedded buffer")
    byte_length = buffers[0].get("byteLength")
    if type(byte_length) is not int or byte_length <= 0 or not 0 <= len(binary) - byte_length <= 3:
        raise ValueError("GLB buffer length does not match the binary chunk")
    views = _list(document, "bufferViews")
    for view in views:
        offset, size = view.get("byteOffset", 0), view.get("byteLength")
        if (view.get("buffer") != 0 or type(offset) is not int or type(size) is not int
                or offset < 0 or size <= 0 or offset + size > byte_length):
            raise ValueError("GLB buffer view exceeds the embedded buffer")
    accessors = _list(document, "accessors")
    for accessor in accessors:
        count = accessor.get("count")
        if not _index(accessor.get("bufferView"), views) or type(count) is not int or count <= 0:
            raise ValueError("GLB accessor must use embedded geometry")
        if "sparse" in accessor:
            raise ValueError("Static profile does not support sparse accessors")

    images = _list(document, "images")
    for image in images:
        if ("uri" in image or image.get("mimeType") not in {"image/png", "image/jpeg"}
                or not _index(image.get("bufferView"), views)):
            raise ValueError("Static GLB images must be embedded PNG or JPEG")
    textures = _list(document, "textures")
    for texture in textures:
        if not _index(texture.get("source"), images):
            raise ValueError("Static GLB textures require embedded images")
    materials = _list(document, "materials")
    textured_materials = set()
    for index, material in enumerate(materials):
        if any(field in material for field in ("normalTexture", "occlusionTexture", "emissiveTexture")):
            raise ValueError("Static profile only supports base-color textures")
        emission = material.get("emissiveFactor", [0, 0, 0])
        if material.get("alphaMode", "OPAQUE") != "OPAQUE" or not _vector(emission, 3) or any(emission):
            raise ValueError("Static profile only supports opaque, non-emissive materials")
        color = material.get("pbrMetallicRoughness", {})
        if not isinstance(color, dict) or "metallicRoughnessTexture" in color:
            raise ValueError("Static profile only supports base-color textures")
        if "baseColorFactor" in color and (not _vector(color["baseColorFactor"], 4)
                or any(not 0 <= value <= 1 for value in color["baseColorFactor"])):
            raise ValueError("Base color must be four numbers in the 0–1 range")
        if "baseColorTexture" in color:
            texture = color["baseColorTexture"]
            if (not isinstance(texture, dict) or not _index(texture.get("index"), textures)
                    or texture.get("texCoord", 0) != 0):
                raise ValueError("Base-color textures must use the first UV map")
            textured_materials.add(index)

    meshes = _list(document, "meshes")
    if not 1 <= len(meshes) <= MAX_SCENE_ENTRIES:
        raise ValueError("Static GLB requires 1–10000 meshes")
    mesh_ids = set()
    for mesh in meshes:
        extras = mesh.get("extras")
        ident = extras.get(MESH_ID) if isinstance(extras, dict) else None
        if not valid_mesh_id(ident) or ident in mesh_ids:
            raise ValueError("Each GLB mesh must contain a unique Game Engine mesh ID")
        mesh_ids.add(ident)
        primitives = _list(mesh, "primitives")
        if not primitives:
            raise ValueError("Each GLB mesh must contain triangles")
        for primitive in primitives:
            if primitive.get("mode", 4) != 4 or "targets" in primitive:
                raise ValueError("Static profile requires triangles without morph targets")
            attributes = primitive.get("attributes")
            if not isinstance(attributes, dict) or any(
                not _index(attributes.get(field), accessors) for field in ("POSITION", "NORMAL")
            ):
                raise ValueError("Static mesh primitives require positions and normals")
            if "indices" in primitive and not _index(primitive["indices"], accessors):
                raise ValueError("Static mesh indices must use an embedded accessor")
            material = primitive.get("material")
            if "material" in primitive and not _index(material, materials):
                raise ValueError("Static mesh material index is invalid")
            if material in textured_materials and not _index(attributes.get("TEXCOORD_0"), accessors):
                raise ValueError("A textured mesh primitive requires the first UV map")
    if mesh_ids != set(expected_ids):
        raise ValueError("Blender did not export the selected mesh IDs exactly once")

    nodes = _list(document, "nodes")
    for node in nodes:
        if "skin" in node or "camera" in node:
            raise ValueError("Static profile does not support skinned or camera objects")
        for field, length in (("matrix", 16), ("translation", 3), ("rotation", 4), ("scale", 3)):
            if field in node and not _vector(node[field], length):
                raise ValueError(f"Static node has invalid {field}")
        if "mesh" in node and not _index(node["mesh"], meshes):
            raise ValueError("Static node mesh index is invalid")
        children = node.get("children", [])
        if not isinstance(children, list) or any(not _index(child, nodes) for child in children):
            raise ValueError("Static node child index is invalid")
    return mesh_ids


def _strip_extras(document):
    # Blender's exporter has a single custom-properties switch for the file.
    # Only mesh identity and the add-on's provenance belong in the runtime asset.
    remaining = [document]
    while remaining:
        item = remaining.pop()
        if isinstance(item, dict):
            item.pop("extras", None)
            remaining.extend(item.values())
        elif isinstance(item, list):
            remaining.extend(item)


def _write_glb(stream, document, binary):
    encoded = json.dumps(document, ensure_ascii=False, allow_nan=False, separators=(",", ":")).encode("utf-8")
    encoded += b" " * (-len(encoded) % 4)
    length = HEADER.size + 2 * CHUNK_HEADER.size + len(encoded) + len(binary)
    if len(encoded) > MAX_JSON_BYTES or length > MAX_GLB_BYTES:
        raise ValueError("Published GLB exceeds the export size limit")
    stream.write(HEADER.pack(b"glTF", 2, length))
    stream.write(CHUNK_HEADER.pack(len(encoded), b"JSON"))
    stream.write(encoded)
    stream.write(CHUNK_HEADER.pack(len(binary), b"BIN\0"))
    stream.write(binary)


def publish_static_glb(destination, export_candidate, expected_ids, provenance):
    """Run an exporter in a sibling staging directory and atomically publish its result."""
    destination = Path(destination)
    if destination.suffix.lower() != ".glb" or destination.is_symlink():
        raise ValueError("Select a .glb destination that is a regular file")
    with TemporaryDirectory(
        prefix=".game-engine-export-", dir=destination.parent, ignore_cleanup_errors=True,
    ) as staging:
        raw = Path(staging) / "candidate.glb"
        export_candidate(raw)
        document, binary = read_glb(raw)
        validate_static_glb(document, binary, expected_ids)
        ids = [mesh["extras"][MESH_ID] for mesh in document["meshes"]]
        _strip_extras(document)
        for mesh, ident in zip(document["meshes"], ids, strict=True):
            mesh["extras"] = {MESH_ID: ident}
        digest = hashlib.sha256()
        binary_digest = hashlib.sha256(binary).hexdigest()
        digest.update(json.dumps(document, ensure_ascii=False, allow_nan=False, separators=(",", ":")).encode("utf-8"))
        digest.update(binary)
        document["asset"]["extras"] = {PROVENANCE: {
            **provenance,
            "profile_version": PROFILE_VERSION,
            "content_sha256": digest.hexdigest(),
            "embedded_data_sha256": binary_digest,
        }}
        published = Path(staging) / "validated.glb"
        with published.open("xb") as stream:
            _write_glb(stream, document, binary)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(published, destination)
    return digest.hexdigest()
