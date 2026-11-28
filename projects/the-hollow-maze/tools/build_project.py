"""Create a fresh Hollow Maze project from verified local source assets."""

import argparse
from array import array
import hashlib
import io
import json
import math
from pathlib import Path
from random import Random
import struct
from uuid import UUID, uuid5
import sys
import wave
import zlib

ROWS = (
    "###########",
    "#PH.....#E#",
    "#.###.#.#.#",
    "#..1#...#H#",
    "##..###...#",
    "#...#2..#.#",
    "#.#...#.#.#",
    "#3..H.M...#",
    "###########",
)
TILE = 2.4
SCENE = "scenes/maze.json"
KNIGHT = "actors/knight.glb"
WARDEN = "actors/warden.glb"
MASONRY = "architecture/masonry.glb"
MUSIC = "audio/beneath-the-stones.wav"
SKY = "environment/ember-sky.png"
NAMESPACE = UUID("46b96f10-b9c9-4cb2-aa04-728416b786bd")
PINS = {
    KNIGHT: (
        "47d080bb44cbe1ca7f561bce99f4a600fe167f05705274716220e2bfae5b52a6",
        "Knight of the Oath",
        10,
    ),
    WARDEN: (
        "06567535b2054507ccf9013c60887820c74fa77396975f62397f0fe8641554a8",
        "The Cinder Warden",
        10,
    ),
    MASONRY: (
        "b4359162073c6647a02dd76944c88fd75e2a4432912c14e2af40d50300462a83",
        "Vault masonry",
        1,
    ),
}

def asset_id(path):
    return str(uuid5(NAMESPACE, path))

def checksum(contents):
    return hashlib.sha256(contents).hexdigest()

def json_bytes(value):
    return (json.dumps(value, indent=2) + "\n").encode("utf-8")

def component(kind, *, component_id=None, **values):
    return {
        "id": component_id or kind,
        "schema_version": 1,
        "enabled": True,
        "data": {"kind": kind, **values},
    }

def object_record(key, name, position, components, scale=(1, 1, 1), rotation=(0, 0, 0)):
    return {
        "key": key,
        "name": name,
        "transform": {"position": position, "rotation": rotation, "scale": scale},
        "components": components,
    }

def model(path, color=None):
    values = {"asset_id": asset_id(path), "model_path": path, "mesh_index": 0}
    if color is not None:
        values["material"] = {"base_color_factor": [*color, 1]}
    return component("mesh_renderer", component_id="mesh", **values)

def collider(layer, mask, half_extents=(0.5, 0.5, 0.5), center=(0, 0, 0)):
    return component("collider", collider={
        "kind": "box",
        "half_extents": half_extents,
        "offset": center,
        "interaction": {"sensor": False, "layers": layer, "mask": mask},
    })

def actor(path, layer, wall_layer):
    return [
        model(path),
        collider(layer, wall_layer, (0.31, 0.65, 0.29), (0, 0.68, 0)),
        component("animation", settings={
            "source": {
                "asset_id": asset_id(path),
                "expected_sha256": PINS[path][0],
                "scene_index": 0,
                "mesh_node": 0,
                "skin_index": 0,
            },
            "playback": {"kind": "single_clip", "clip_index": 0, "looping": True},
            "speed_numerator": 1,
            "speed_denominator": 1,
        }),
    ]

def soundtrack():
    """Render a quiet, seamless loop of triangle and square waves to PCM."""
    rate = 16000
    beat_samples = rate // 2
    melody = (45, 0, 52, 0, 56, 0, 53, 0, 48, 0, 55, 0, 53, 0, 52, 0)
    bass = (33, 33, 29, 29, 36, 36, 31, 31)
    pcm = array("h")
    for beat in range(32):
        lead = melody[beat % len(melody)]
        high = 440 * 2 ** ((lead - 69) / 12)
        low = 440 * 2 ** ((bass[beat // 4] - 69) / 12)
        for sample in range(beat_samples):
            time = sample / rate
            envelope = min(1.0, time * 44, (0.5 - time) * 18)
            triangle = 1 - 4 * abs((time * low) % 1 - 0.5)
            square = 1 if (time * high) % 1 < 0.28 else -1
            lead_envelope = max(0, 1 - time * 3)
            value = 0.28 * triangle + (0.09 * square * lead_envelope if lead else 0)
            pcm.append(round(value * envelope * 32767))
    if sys.byteorder != "little":
        pcm.byteswap()
    buffer = io.BytesIO()
    with wave.open(buffer, "wb") as output:
        output.setparams((1, 2, rate, 0, "NONE", "not compressed"))
        output.writeframes(pcm.tobytes())
    return buffer.getvalue()

def ember_sky():
    """Bake seamless red cloud banks into a small spherical PNG."""
    width, height = 768, 384
    random = Random(48627)

    def cloud_layer(scale):
        side = 2 * scale + 2
        plane = side * side
        noise = [random.random() for _ in range(side * plane)]

        def sample(x, y, z):
            px, py, pz = ((value + 1) * scale for value in (x, y, z))
            ix, iy, iz = int(px), int(py), int(pz)
            fx, fy, fz = px - ix, py - iy, pz - iz
            tx = fx * fx * (3 - 2 * fx)
            ty = fy * fy * (3 - 2 * fy)
            tz = fz * fz * (3 - 2 * fz)
            index = ix + side * iy + plane * iz
            front0 = noise[index] * (1 - tx) + noise[index + 1] * tx
            front1 = noise[index + side] * (1 - tx) + noise[index + side + 1] * tx
            back0 = noise[index + plane] * (1 - tx) + noise[index + plane + 1] * tx
            back1 = noise[index + plane + side] * (1 - tx) + noise[index + plane + side + 1] * tx
            return ((front0 * (1 - ty) + front1 * ty) * (1 - tz)
                    + (back0 * (1 - ty) + back1 * ty) * tz)

        return sample

    broad, rolling, wisps = (cloud_layer(scale) for scale in (3, 8, 18))
    longitudes = [((column + 0.5) / width - 0.5) * math.tau for column in range(width)]
    bearings = [(math.sin(angle), -math.cos(angle)) for angle in longitudes]
    dark, red, ember = (28, 9, 19), (129, 48, 36), (245, 157, 86)
    pixels = bytearray()
    for row in range(height):
        polar = (row + 0.5) * math.pi / height
        radial, y = math.sin(polar), math.cos(polar)
        pixels.append(0)  # PNG scanline filter None
        for east, north in bearings:
            x, z = east * radial, north * radial
            cloud = 0.62 * broad(x, y, z) + 0.27 * rolling(x, y, z) + 0.11 * wisps(x, y, z)
            density = min(1, max(0, (cloud - 0.15) / 0.69))
            if density < 0.6:
                low, high, blend = dark, red, density / 0.6
            else:
                low, high, blend = red, ember, ((density - 0.6) / 0.4) ** 1.25
            pixels.extend(round(a + (b - a) * blend) for a, b in zip(low, high))

    def chunk(tag, payload):
        return (struct.pack(">I", len(payload)) + tag + payload
                + struct.pack(">I", zlib.crc32(tag + payload)))

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
            + chunk(b"IDAT", zlib.compress(pixels, 9)) + chunk(b"IEND", b""))

def make_scene():
    entities = []
    membership = {}
    folders = {
        "characters": "Characters",
        "maze-stone": "Maze Walls",
        "hurdles": "Jump Barriers",
        "floor": "Floor",
        "mechanisms": "Seals and Gate",
        "torches": "Wall Torches",
        "atmosphere": "Camera, Lighting and Music",
    }

    def add(folder, key, name, position, components, scale=(1, 1, 1), rotation=(0, 0, 0), *, parent=None):
        record = object_record(key, name, position, components, scale, rotation)
        if parent is None:
            membership[key] = folder
        else:
            record["parent"] = parent
        entities.append(record)

    def stone(folder, key, name, position, scale, color, rotation=(0, 0, 0), solid=False, *, parent=None):
        components = [model(MASONRY, color)]
        if solid:
            components.append(collider(2, 12))
        add(folder, key, name, position, components, scale, rotation, parent=parent)

    def light(key, name, kind, position, color, intensity, distance, rotation=(0, 0, 0)):
        add("atmosphere", key, name, position, [
            component("light", settings={
                "kind": kind, "color": color, "intensity": intensity, "range": distance,
            }),
        ], rotation=rotation)

    def emitter(rate, lifetime, speed, spread, gravity, size, color, seed):
        return component("particle_emitter", component_id="particle-emitter", settings={
            "rate_per_second": rate, "lifetime_seconds": lifetime, "speed": speed,
            "spread_degrees": spread, "gravity": gravity, "size": size,
            "color": color, "seed": seed,
        })

    def torch(index, column, row, inward_x, inward_z):
        key = f"wall-torch-{index:02}"
        x = (column - (width - 1) / 2) * TILE + 0.91 * inward_x
        z = (row - (height - 1) / 2) * TILE + 0.91 * inward_z
        add("torches", key, f"Wall Torch {index:02}", (x, 0.97, z), [])
        stone("torches", f"{key}-mount", "Iron Mount", (0, 0.02, 0),
              (0.39, 0.23, 0.39), (0.075, 0.054, 0.042), parent=key)
        stone("torches", f"{key}-stem", "Torch Stem", (0, 0.29, 0),
              (0.15, 0.48, 0.15), (0.18, 0.080, 0.038), parent=key)
        stone("torches", f"{key}-bowl", "Copper Brazier", (0, 0.54, 0),
              (0.46, 0.18, 0.46), (0.43, 0.21, 0.062), parent=key)
        add("torches", f"{key}-flame", "Flame", (0, 0.65, 0), [
            emitter(48, 0.63, 1.55, 36, 0.65, 0.48, (1.0, 0.34, 0.025), 121 + 29 * index),
            component("light", settings={
                "kind": "point", "color": (1.0, 0.43, 0.15), "intensity": 3.25, "range": 3.8,
            }),
        ], parent=key)
        add("torches", f"{key}-embers", "Embers", (0, 0.78, 0), [
            emitter(12, 1.02, 2.25, 42, 0.42, 0.18, (1.0, 0.70, 0.065), 281 + 29 * index),
        ], parent=key)

    bindings = {"player": "knight", "monster": "hollow-warden", "gate": "sealed-gate"}
    bindings.update({f"seal_{n}": f"seal-{n}" for n in (1, 2, 3)})
    width, height = len(ROWS[0]), len(ROWS)
    stone("floor", "foundation", "Basalt Foundation", (0, -0.39, 0),
          (width * TILE + 0.45, 0.62, height * TILE + 0.45), (0.035, 0.068, 0.084))
    floor_colors = ((0.20, 0.27, 0.27), (0.25, 0.31, 0.29), (0.29, 0.33, 0.30))
    seal_colors = ((0.20, 0.86, 0.94), (0.60, 0.91, 0.42), (0.93, 0.63, 0.28))
    for row, line in enumerate(ROWS):
        for col, tile in enumerate(line):
            x, z = (col - (width - 1) / 2) * TILE, (row - (height - 1) / 2) * TILE
            suffix = f"{row}-{col}"
            if tile == "#":
                stone("maze-stone", f"wall-{suffix}", f"Wall {row}.{col}", (x, 0.44, z),
                      (TILE, 0.92, TILE), (0.095, 0.15, 0.22), solid=True)
                crown_height = 0.19 if (col + row) % 4 == 0 else 0.11
                stone("maze-stone", f"crown-{suffix}", f"Wall Cap {row}.{col}",
                      (x, 0.90 + crown_height / 2, z),
                      (TILE - 0.08, crown_height, TILE - 0.08), (0.20, 0.29, 0.39))
                continue
            color = floor_colors[(row * 2 + col) % len(floor_colors)]
            stone("floor", f"tile-{suffix}", f"Flagstone {row}.{col}", (x, -0.04, z),
                  (TILE - 0.082, 0.08, TILE - 0.082), color)
            if tile == "H":
                crosses_east_west = ROWS[row][col - 1] != "#" and ROWS[row][col + 1] != "#"
                rotation = (0, 0 if crosses_east_west else math.pi / 2, 0)
                stone("hurdles", f"hurdle-{suffix}", "Low Jump Barrier", (x, 0.24, z),
                      (0.58, 0.48, 1.96), (0.28, 0.16, 0.08), rotation, solid=True)
                stone("hurdles", f"hurdle-rail-{suffix}", "Brass Barrier Rail", (x, 0.53, z),
                      (0.64, 0.10, 2.02), (0.89, 0.69, 0.24), rotation)
            elif tile == "P":
                add("characters", "knight", "Knight", (x, 0.02, z),
                    actor(KNIGHT, 4, 2), (0.86, 0.86, 0.86), (0, -math.pi / 2, 0))
            elif tile == "M":
                add("characters", "hollow-warden", "Hollow Warden", (x, 0.02, z),
                    actor(WARDEN, 8, 2), (0.99, 0.99, 0.99), (0, math.pi / 2, 0))
            elif tile in "123":
                color = seal_colors[int(tile) - 1]
                stone("mechanisms", f"plinth-{tile}", f"Seal Plinth {tile}", (x, 0.11, z),
                      (1.05, 0.22, 1.05), (0.08, 0.19, 0.21), (0, math.pi / 4, 0))
                stone("mechanisms", f"seal-{tile}", f"Rune Seal {tile}", (x, 0.37, z),
                      (0.68, 0.31, 0.68), color, (0, math.pi / 4, 0))
                light(f"seal-light-{tile}", f"Seal Light {tile}", "point",
                      (x, 1.45, z), color, 2.2, 2.6)
            elif tile == "E":
                stone("mechanisms", "exit-marker", "Exit Threshold", (x, 0.055, z),
                      (1.35, 0.11, 1.35), (0.17, 0.74, 0.52), (0, math.pi / 4, 0))
                stone("mechanisms", "sealed-gate", "Sealed Gate", (x, 0.66, z),
                      (1.48, 1.21, 0.47), (0.78, 0.31, 0.11), solid=True)
                light("exit-light", "Exit Light", "point", (x, 2.35, z),
                      (1.0, 0.60, 0.28), 4.2, 4.2)
    torch_positions = (
        (3, 0, 0, 1), (7, 0, 0, 1), (10, 3, -1, 0), (10, 6, -1, 0),
        (7, 8, 0, -1), (3, 8, 0, -1), (0, 5, 1, 0), (0, 2, 1, 0),
    )
    for index, (column, row, inward_x, inward_z) in enumerate(torch_positions, start=1):
        torch(index, column, row, inward_x, inward_z)
    add("atmosphere", "maze-camera", "Maze Overview", (0, 29, 8.4), [
        component("camera", settings={
            "lens": {"vertical_fov": 0.81, "near": 0.1, "far": 80.0},
            "rig": {"mode": "fixed"},
        }),
    ], rotation=(-math.atan2(29, 8.4), 0, 0))
    add("atmosphere", "crypt-sky", "Ember Sky", (0, 0, 0), [
        component("sky", settings={
            "zenith": (0.09, 0.031, 0.062),
            "horizon": (0.34, 0.115, 0.19),
            "yaw_degrees": 0,
            "background": {"kind": "panorama", "image": asset_id(SKY)},
        }),
    ])
    add("atmosphere", "dungeon-music", "Beneath the Stones", (0, 0, 0), [
        component("audio_source", component_id="ambience", settings={
            "clip": {"asset": asset_id(MUSIC)}, "gain": 0.42, "loop": True,
            "playback": "on_scene_enter", "bus": "music",
        }),
    ])
    light("moonlight", "Cold Overhead Light", "directional", (0, 7, 0),
          (0.80, 0.87, 1.0), 1.15, 80, (-1.12, -0.48, 0))
    light("warm-lantern", "Warm Lantern Light", "point", (-7.5, 3.4, 5.9),
          (1.0, 0.51, 0.27), 3.25, 9)
    return {
        "schema_version": 8,
        "bindings": bindings,
        "entities": entities,
        "default_game_camera": "maze-camera",
        "organization": {
            "folders": [{"id": key, "name": name} for key, name in folders.items()],
            "root_membership": membership,
        },
        "environment": {"active_sky": "crypt-sky", "ambient_linear": (0.28, 0.35, 0.45)},
    }

def prepare(donor_assets):
    sources = {}
    records = []
    provenance = []
    for path, (digest, mesh_name, primitives) in PINS.items():
        payload = (donor_assets / path).read_bytes()
        if checksum(payload) != digest:
            raise ValueError(f"Source digest differs from its pin for {path}")
        sources[path] = payload
        records.append({
            "id": asset_id(path), "path": path, "kind": "model", "revision": 1,
            "bytes": len(payload), "fingerprint": digest,
            "meshes": [{"index": 0, "name": mesh_name, "primitives": primitives}],
        })
        provenance.append({
            "origin": f"projects/cinder-oath/assets/{path}", "destination": f"assets/{path}",
            "sha256": digest, "bytes": len(payload),
        })
    music = soundtrack()
    sources[MUSIC] = music
    records.append({
        "id": asset_id(MUSIC), "path": MUSIC, "kind": "audio_clip", "revision": 1,
        "bytes": len(music), "fingerprint": checksum(music),
    })
    sky = ember_sky()
    sources[SKY] = sky
    records.append({
        "id": asset_id(SKY), "path": SKY, "kind": "sky_panorama", "revision": 1,
        "bytes": len(sky), "fingerprint": checksum(sky),
    })
    sources[SCENE] = json_bytes(make_scene())
    sources["asset-catalog.json"] = json_bytes({"schema_version": 1, "assets": records})
    project = {
        "schema_version": 3,
        "name": "The Hollow Maze",
        "gameplay_module": "knight_maze",
        "fixed_hz": 60,
        "max_catchup_steps": 8,
        "behaviors": [
            {"scene_id": "maze", "behavior": {
                "kind": "controlled_move", "entity": "knight", "speed": 2.4, "space": "world",
            }},
        ],
        "scenes": [{"id": "maze", "asset_path": SCENE}],
        "startup_scene_id": "maze",
        "input_profile": {
            "schema_version": 1,
            "movement": {"negative_x": "a", "positive_x": "d", "negative_y": "s", "positive_y": "w"},
            "actions": [
                {"id": "step", "key": "e"},
                {"id": "jump", "key": "space"},
                {"id": "wait", "key": "q"},
            ],
            "mouse_actions": [
                {"id": "step", "button": "left"},
                {"id": "jump", "button": "right"},
            ],
        },
        "gameplay_settings": {"schema_version": 1, "scene": "maze", "rows": ROWS, "tile_size": TILE},
    }
    return {
        "project.json": json_bytes(project),
        "asset-reuse.json": json_bytes({"schema_version": 1, "sources": provenance}),
        **{f"assets/{path}": data for path, data in sources.items()},
    }

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path, help="new output directory")
    parser.add_argument("--donor-assets", type=Path,
                        default=Path(__file__).resolve().parents[2] / "cinder-oath" / "assets")
    options = parser.parse_args()
    if options.output.exists():
        parser.error("--output must be a new directory")
    try:
        files = prepare(options.donor_assets)
        options.output.mkdir()
        for relative, data in files.items():
            path = options.output / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open("xb") as output:
                output.write(data)
    except (OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")

if __name__ == "__main__":
    main()
