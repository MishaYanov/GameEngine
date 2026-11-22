"""Build Cinder Oath's original faceted, skinned actors as embedded GLB files."""

from collections import defaultdict
from hashlib import sha256
import json
import math
from pathlib import Path
import struct
import uuid


ACTORS = Path(__file__).resolve().parents[1] / "assets" / "actors"
PALETTE = {
    "armor": ("Forged steel", (0.29, 0.38, 0.45)),
    "crest": ("Silver bevels", (0.62, 0.72, 0.76)),
    "shade": ("Burnished seams", (0.11, 0.15, 0.18)),
    "lining": ("Padded leather", (0.078, 0.052, 0.040)),
    "brass": ("Hammered brass", (0.57, 0.37, 0.15)),
    "cloth": ("Oath-blue cloth", (0.035, 0.21, 0.27)),
    "trim": ("Blue silk edging", (0.09, 0.34, 0.37)),
    "visor": ("Closed visor", (0.018, 0.024, 0.027)),
    "signal": ("Pale teal", (0.53, 0.86, 0.85)),
    "blade": ("Blade edge", (0.78, 0.86, 0.89)),
}
WARDEN = {
    "armor": ("Black iron", (0.15, 0.125, 0.117)),
    "crest": ("Heat-stained bevels", (0.36, 0.24, 0.19)),
    "shade": ("Blackened seams", (0.086, 0.066, 0.060)),
    "brass": ("Oxidized copper", (0.68, 0.28, 0.095)),
    "cloth": ("Maroon cloth", (0.28, 0.058, 0.032)),
    "trim": ("Brick silk edging", (0.44, 0.125, 0.060)),
    "signal": ("Burning slit", (1.00, 0.37, 0.095)),
}


def add(a, b):
    return tuple(x + y for x, y in zip(a, b))


def sub(a, b):
    return tuple(x - y for x, y in zip(a, b))


def scale(a, amount):
    return tuple(x * amount for x in a)


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def cross(a, b):
    x, y, z = a
    p, q, r = b
    return y * r - z * q, z * p - x * r, x * q - y * p


def unit(a):
    return scale(a, 1 / math.sqrt(dot(a, a)))


def average(vertices):
    return tuple(sum(values) / len(vertices) for values in zip(*vertices))


def quat_product(a, b):
    x, y, z, w = a
    u, v, t, s = b
    return (w*u + x*s + y*t - z*v, w*v - x*t + y*s + z*u,
            w*t + x*v - y*u + z*s, w*s - x*u - y*v - z*t)


def rotation(x=0, y=0, z=0):
    a, b, c = (math.radians(v) / 2 for v in (x, y, z))
    rx = math.sin(a), 0, 0, math.cos(a)
    ry = 0, math.sin(b), 0, math.cos(b)
    rz = 0, 0, math.sin(c), math.cos(c)
    return quat_product(quat_product(ry, rx), rz)


class Actor:
    def __init__(self, name, palette, size=1.0):
        self.name = name
        self.palette = palette
        self.size = size
        self.bones = {}
        self.geometry = defaultdict(list)
        self.animations = {}

    def bone(self, name, parent, position):
        self.bones[name] = (parent, scale(position, self.size), len(self.bones))

    def polygon(self, material, bone, points, outward):
        normal = unit(cross(sub(points[1], points[0]), sub(points[2], points[0])))
        if dot(normal, outward) < 0:
            points = points[::-1]
            normal = scale(normal, -1)
        joint = self.bones[bone][2]
        for i in range(1, len(points) - 1):
            self.geometry[material].extend(
                (scale(point, self.size), normal, joint)
                for point in (points[0], points[i], points[i + 1])
            )

    def rings(self, material, bone, levels):
        # Octagonal cross-sections create a bevel without smoothing the armor planes.
        rings = []
        for y, cx, cz, hx, hz in levels:
            rings.append([(cx + hx*x, y, cz + hz*z) for x, z in (
                (-0.7, -1), (0.7, -1), (1, -0.7), (1, 0.7),
                (0.7, 1), (-0.7, 1), (-1, 0.7), (-1, -0.7),
            )])
        self.tube(material, bone, rings)

    def tube(self, material, bone, rings):
        middle = average([point for ring in rings for point in ring])
        self.polygon(material, bone, rings[0], sub(average(rings[0]), middle))
        self.polygon(material, bone, rings[-1], sub(average(rings[-1]), middle))
        for lower, upper in zip(rings, rings[1:]):
            for i in range(len(lower)):
                j = (i + 1) % len(lower)
                points = [lower[i], lower[j], upper[j], upper[i]]
                self.polygon(material, bone, points, sub(average(points), middle))

    def box(self, material, bone, center, half):
        x, y, z = center
        hx, hy, hz = half
        self.rings(material, bone, [(y-hy, x, z, hx, hz), (y+hy, x, z, hx, hz)])

    def segment(self, material, bone, start, end, width, end_width, depth=1, sides=6):
        direction = unit(sub(end, start))
        reference = (0, 0, 1) if abs(direction[2]) < 0.9 else (0, 1, 0)
        side = unit(cross(direction, reference))
        other = unit(cross(side, direction))
        rings = [
            [add(at, add(scale(side, w*math.cos(i*math.tau/sides)),
                         scale(other, w*depth*math.sin(i*math.tau/sides))))
             for i in range(sides)]
            for at, w in [(start, width), (end, end_width)]
        ]
        self.tube(material, bone, rings)

    def plate(self, material, bone, outline, face, back):
        # The supplied outline is an XY silhouette, with a visible -Z ridge.
        front = [(x, y, face) for x, y in outline]
        rear = [(x, y, back) for x, y in outline]
        center = (*average(outline), face - 0.025)
        for i in range(len(outline)):
            j = (i + 1) % len(outline)
            self.polygon(material, bone, [front[i], front[j], center], (0, 0, -1))
            edge = [rear[i], rear[j], front[j], front[i]]
            self.polygon(material, bone, edge, sub(average(edge), average(front)))
        self.polygon(material, bone, rear, (0, 0, 1))

    def sword(self, bone, wrist, direction, length, guard_width, heavy=False):
        direction = unit(direction)
        side = unit(cross(direction, (0, 0, -1)))
        toward = unit(cross(side, direction))

        def at(along, across=0, ridge=0):
            return add(wrist, add(scale(direction, along),
                                  add(scale(side, across), scale(toward, ridge))))

        self.segment("lining", bone, at(-0.15), at(0.035), 0.032, 0.031, sides=8)
        self.segment("brass", bone, at(-0.18), at(-0.135), 0.042, 0.035, sides=8)
        self.segment("shade", bone, at(-0.143), at(-0.119), 0.041, 0.039, sides=8)
        for offset in (-0.10, -0.067, -0.034, 0):
            self.segment("shade", bone, at(offset), at(offset+0.008), 0.035, 0.035)
        for sign in (-1, 1):
            self.segment("brass", bone, at(0.038), at(0.079, sign*guard_width), 0.031, 0.024)
            self.segment("crest", bone, at(0.072, sign*guard_width), at(0.101, sign*guard_width),
                         0.027, 0.013, sides=5)
        self.segment("crest", bone, at(0.051), at(0.125), 0.044, 0.050 if heavy else 0.040)
        edge = 0.066 if heavy else 0.049
        points = [(0.105, edge), (length-0.16, edge*0.76), (length, 0.003)]
        for (a, width_a), (b, width_b) in zip(points, points[1:]):
            for sign in (-1, 1):
                for front in (-1, 1):
                    outward = scale(toward, front)
                    inner_a = min(width_a*0.58, 0.033)
                    inner_b = min(width_b*0.58, 0.033)
                    self.polygon("crest", bone, [
                        at(a, 0, front*0.019), at(b, 0, front*0.013),
                        at(b, sign*inner_b, front*0.007), at(a, sign*inner_a, front*0.009),
                    ], outward)
                    self.polygon("blade", bone, [
                        at(a, sign*inner_a, front*0.009), at(b, sign*inner_b, front*0.007),
                        at(b, sign*width_b), at(a, sign*width_a),
                    ], outward)
        for front in (-1, 1):
            self.polygon("shade", bone, [
                at(0.16, 0.005, front*0.020), at(0.16, -0.005, front*0.020),
                at(length-0.17, 0, front*0.015),
            ], scale(toward, front))

    def motion(self, clip, bone, values, kind="rotation"):
        values = [(time, rotation(*value) if kind == "rotation" else scale(value, self.size))
                  for time, value in values]
        self.animations.setdefault(clip, []).append((bone, kind, values))


class Glb:
    def __init__(self):
        self.data = bytearray()
        self.views = []
        self.accessors = []

    def accession(self, rows, component, dimension, bounds=False, target=None):
        rows = list(rows)
        fmt = {5121: "B", 5126: "f"}[component]
        self.data.extend(b"\0" * (-len(self.data) % 4))
        offset = len(self.data)
        geometry = struct.Struct("<" + fmt*len(rows[0]))
        for row in rows:
            self.data.extend(geometry.pack(*row))
        view = {"buffer": 0, "byteOffset": offset, "byteLength": len(self.data) - offset}
        if target is not None:
            view["target"] = target
        self.views.append(view)
        accessor = {"bufferView": len(self.views) - 1, "componentType": component,
                    "count": len(rows), "type": dimension}
        if bounds:
            columns = list(zip(*rows))
            accessor.update(min=[min(v) for v in columns], max=[max(v) for v in columns])
        self.accessors.append(accessor)
        return len(self.accessors) - 1

    def serialize(self, actor):
        nodes = [{"name": actor.name, "mesh": 0, "skin": 0}]
        inverse_binds = []
        scene_nodes = [0]
        for name, (parent, position, _) in actor.bones.items():
            if parent is None:
                translation = position
                scene_nodes.append(len(nodes))
            else:
                _, parent_position, parent_joint = actor.bones[parent]
                nodes[parent_joint + 1].setdefault("children", []).append(len(nodes))
                translation = sub(position, parent_position)
            nodes.append({"name": name, "translation": translation})
            x, y, z = position
            inverse_binds.append((1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, -x, -y, -z, 1))
        inverse_accessor = self.accession(inverse_binds, 5126, "MAT4")
        materials = []
        primitives = []
        for key, vertices in actor.geometry.items():
            position, normal, joint = zip(*vertices)
            name, color = actor.palette[key]
            materials.append({"name": name, "pbrMetallicRoughness": {
                "baseColorFactor": [*color, 1], "metallicFactor": 0.1, "roughnessFactor": 0.62,
            }})
            primitives.append({"material": len(materials)-1, "attributes": {
                "POSITION": self.accession(position, 5126, "VEC3", bounds=True, target=34962),
                "NORMAL": self.accession(normal, 5126, "VEC3", target=34962),
                "JOINTS_0": self.accession(((j, 0, 0, 0) for j in joint), 5121, "VEC4", target=34962),
                "WEIGHTS_0": self.accession(((1, 0, 0, 0) for _ in joint), 5126, "VEC4", target=34962),
            }})

        animations = []
        for name in ("idle", "walk", "attack", "defeat"):
            animation = {"name": name, "samplers": [], "channels": []}
            for bone, path, keyframes in actor.animations[name]:
                times, values = zip(*keyframes)
                sampler = {
                    "input": self.accession(((time,) for time in times), 5126, "SCALAR", bounds=True),
                    "output": self.accession(values, 5126, "VEC4" if path == "rotation" else "VEC3"),
                }
                animation["channels"].append({"sampler": len(animation["samplers"]), "target": {
                    "node": actor.bones[bone][2] + 1, "path": path,
                }})
                animation["samplers"].append(sampler)
            animations.append(animation)

        self.data.extend(b"\0" * (-len(self.data) % 4))
        document = {
            "asset": {"version": "2.0", "generator": "Cinder Oath actor workshop"},
            "scene": 0,
            "scenes": [{"nodes": scene_nodes}],
            "nodes": nodes,
            "skins": [{"inverseBindMatrices": inverse_accessor, "skeleton": 1,
                       "joints": list(range(1, len(nodes)))}],
            "meshes": [{"name": actor.name, "primitives": primitives}],
            "materials": materials,
            "buffers": [{"byteLength": len(self.data)}],
            "bufferViews": self.views,
            "accessors": self.accessors,
            "animations": animations,
        }
        metadata = json.dumps(document, separators=(",", ":"), allow_nan=False).encode("utf-8")
        metadata += b" " * (-len(metadata) % 4)
        return (struct.pack("<4sII", b"glTF", 2, len(metadata)+len(self.data)+28)
                + struct.pack("<I4s", len(metadata), b"JSON") + metadata
                + struct.pack("<I4s", len(self.data), b"BIN\0") + self.data)


def armored_knight(enemy=False):
    actor = Actor("The Cinder Warden" if enemy else "Knight of the Oath",
                  PALETTE | WARDEN if enemy else PALETTE, 1.10 if enemy else 1.0)
    actor.bone("root", None, (0, 0, 0))
    actor.bone("pelvis", "root", (0, 0.86, 0))
    actor.bone("spine", "pelvis", (0, 1.17, 0))
    actor.bone("head", "spine", (0, 1.63, 0))
    actor.bone("mantle", "spine", (0, 1.49, 0.16))
    for suffix, sign in (("L", -1), ("R", 1)):
        actor.bone("arm."+suffix, "spine", (sign*0.31, 1.44, 0))
        actor.bone("elbow."+suffix, "arm."+suffix, (sign*0.425, 1.18, -0.07))
        actor.bone("hand."+suffix, "elbow."+suffix, (sign*0.44, 1.04, -0.27))
        actor.bone("thigh."+suffix, "pelvis", (sign*0.135, 0.84, 0))
        actor.bone("shin."+suffix, "thigh."+suffix, (sign*0.152, 0.48, -0.022))
        actor.bone("foot."+suffix, "shin."+suffix, (sign*0.166, 0.14, 0.012))

    actor.rings("lining", "pelvis", [(0.75, 0, 0.025, 0.21, 0.12), (0.99, 0, 0, 0.24, 0.14)])
    actor.rings("shade", "spine", [(0.98, 0, 0.02, 0.19, 0.14), (1.44, 0, 0.02, 0.25, 0.14)])
    actor.rings("armor", "spine", [(1.055, 0, 0, 0.19, 0.14), (1.18, 0, 0, 0.26, 0.18),
                                    (1.39, 0, 0, 0.31, 0.19), (1.51, 0, 0.022, 0.22, 0.14)])
    actor.plate("crest", "spine", [(-0.205, 1.388), (0.205, 1.388), (0.166, 1.216),
                                  (0, 1.130), (-0.166, 1.216)], -0.185, -0.156)
    actor.plate("shade", "spine", [(-0.101, 1.353), (0.101, 1.353), (0.088, 1.251),
                                  (0, 1.193), (-0.088, 1.251)], -0.207, -0.190)
    actor.plate("brass", "spine", [(0, 1.341), (0.052, 1.268), (0, 1.216), (-0.052, 1.268)],
                -0.231, -0.203)
    actor.plate("cloth", "spine", [(0, 1.319), (0.026, 1.268), (0, 1.244), (-0.026, 1.268)],
                -0.251, -0.217)
    actor.rings("brass", "spine", [(1.488, 0, 0, 0.134, 0.105), (1.548, 0, 0, 0.130, 0.103)])
    actor.rings("lining", "head", [(1.51, 0, 0.01, 0.092, 0.084), (1.65, 0, 0.01, 0.099, 0.087)])
    actor.rings("lining", "pelvis", [(0.961, 0, 0, 0.227, 0.159), (1.023, 0, 0, 0.226, 0.158)])
    actor.box("brass", "pelvis", (0, 0.993, -0.161), (0.055, 0.032, 0.018))
    actor.box("shade", "pelvis", (0, 0.993, -0.183), (0.031, 0.018, 0.010))
    for sign in (-1, 1):
        coords = [(sign*0.069, 0.973), (sign*0.213, 0.975), (sign*0.251, 0.751),
                  (sign*0.144, 0.708), (sign*0.080, 0.779)]
        actor.plate("armor", "pelvis", coords, -0.116, -0.084)
    actor.plate("cloth", "pelvis", [(-0.068, 0.970), (0.068, 0.970), (0.090, 0.654),
                                   (0, 0.590), (-0.090, 0.654)], -0.153, -0.139)
    actor.plate("trim", "pelvis", [(-0.090, 0.665), (0, 0.602), (0.090, 0.665),
                                  (0.083, 0.635), (0, 0.580), (-0.083, 0.635)], -0.161, -0.150)

    actor.rings("armor", "head", [(1.567, 0, 0.012, 0.108, 0.115),
                                  (1.635, 0, 0.018, 0.139, 0.137),
                                  (1.807, 0, 0.020, 0.145, 0.143),
                                  (1.903, 0, 0.023, 0.115, 0.120),
                                  (1.944, 0, 0.027, 0.067, 0.072)])
    actor.plate("shade", "head", [(-0.119, 1.790), (0.119, 1.790), (0.108, 1.658),
                                 (0, 1.587), (-0.108, 1.658)], -0.129, -0.105)
    actor.plate("crest", "head", [(-0.123, 1.797), (-0.091, 1.806), (0.091, 1.806), (0.123, 1.797),
                                 (0.106, 1.783), (-0.106, 1.783)], -0.164, -0.112)
    actor.plate("visor", "head", [(-0.097, 1.771), (0.097, 1.771), (0.083, 1.737),
                                 (0, 1.728), (-0.083, 1.737)], -0.174, -0.140)
    actor.plate("signal", "head", [(-0.068, 1.760), (0.069, 1.760), (0.046, 1.753),
                                  (-0.046, 1.753)], -0.196, -0.170)
    actor.plate("armor", "head", [(-0.106, 1.729), (0, 1.715), (0.106, 1.729),
                                 (0.089, 1.653), (0, 1.619), (-0.089, 1.653)], -0.148, -0.124)
    for x in (-0.044, -0.023, 0, 0.023, 0.044):
        actor.box("visor", "head", (x, 1.674+abs(x)*0.2, -0.168), (0.006, 0.011, 0.006))
    actor.plate("brass", "head", [(0, 1.922), (0.019, 1.876), (0, 1.805), (-0.019, 1.876)],
                -0.136, -0.087)
    if enemy:
        for sign in (-1, 1):
            actor.segment("shade", "head", (sign*0.100, 1.844, 0.031),
                          (sign*0.173, 1.970, 0.045), 0.056, 0.035, sides=5)
            actor.segment("crest", "head", (sign*0.173, 1.962, 0.044),
                          (sign*0.224, 2.072, -0.015), 0.036, 0.002, sides=5)
    else:
        actor.rings("brass", "head", [(1.919, 0, 0.005, 0.040, 0.055),
                                      (1.977, 0, 0.017, 0.031, 0.043)])
        actor.segment("cloth", "head", (0, 1.962, -0.014), (0, 2.035, 0.109), 0.041, 0.068, 0.47)
        actor.segment("trim", "head", (0, 2.029, 0.107), (0.036, 1.965, 0.249), 0.060, 0.009, 0.37)

    # Cape folds have a separate joint so they lag behind the attack wind-up.
    cape_top = [(-0.235, 1.477, 0.151), (-0.09, 1.450, 0.194), (0.09, 1.450, 0.194), (0.235, 1.477, 0.151)]
    cape_bottom = [(-0.303, 0.668, 0.273), (-0.132, 0.620, 0.365),
                   (0.136, 0.600, 0.339), (0.282, 0.689, 0.260)]
    for i in range(3):
        b, n = cape_bottom[i:i+2]
        cap = [add(b, (0, 0.035, 0)), add(n, (0, 0.035, 0))]
        points = [cape_top[i], cape_top[i+1], cap[1], cap[0]]
        trim = [cap[0], cap[1], n, b]
        for side in (-1, 1):
            offset = (0, 0, 0.005*side)
            actor.polygon("cloth" if i != 1 else "trim", "mantle",
                          [add(v, offset) for v in points], (0, 0, side))
            actor.polygon("brass", "mantle", [add(v, offset) for v in trim], (0, 0, side))

    for suffix, sign in (("L", -1), ("R", 1)):
        arm, elbow, hand = ("arm."+suffix, "elbow."+suffix, "hand."+suffix)
        thigh, shin, foot = ("thigh."+suffix, "shin."+suffix, "foot."+suffix)
        actor.segment("lining", arm, (sign*0.29, 1.44, 0), (sign*0.423, 1.175, -0.074), 0.10, 0.073)
        actor.rings("shade", arm, [(1.316, sign*0.365, 0.004, 0.108, 0.133),
                                   (1.452, sign*0.315, 0.013, 0.143, 0.136)])
        actor.rings("armor", arm, [(1.370, sign*0.379, 0.005, 0.119, 0.139),
                                   (1.470, sign*0.337, 0.009, 0.151, 0.141),
                                   (1.535, sign*0.327, 0.014, 0.084, 0.096)])
        actor.rings("crest", arm, [(1.365, sign*0.375, 0.002, 0.122, 0.145),
                                   (1.398, sign*0.368, 0.002, 0.135, 0.144)])
        if enemy:
            for z in (-0.072, 0.062):
                actor.segment("crest", arm, (sign*0.405, 1.468, z),
                              (sign*0.550, 1.553, z+0.011), 0.046, 0.004, sides=4)
        actor.segment("armor", elbow, (sign*0.425, 1.176, -0.067), (sign*0.44, 1.050, -0.246),
                      0.086, 0.062, sides=6)
        actor.segment("crest", elbow, (sign*0.439, 1.073, -0.201), (sign*0.444, 1.046, -0.248),
                      0.069, 0.065, sides=6)
        actor.box("lining", hand, (sign*0.440, 1.036, -0.278), (0.058, 0.054, 0.056))
        actor.box("crest", hand, (sign*0.440, 1.030, -0.310), (0.051, 0.047, 0.026))
        for gap in (-0.029, 0, 0.029):
            actor.box("shade", hand, (sign*0.440+gap, 1.018, -0.330), (0.006, 0.034, 0.007))

        actor.segment("lining", thigh, (sign*0.137, 0.838, 0.021), (sign*0.153, 0.480, -0.02),
                      0.108, 0.078)
        actor.segment("armor", thigh, (sign*0.138, 0.777, -0.018), (sign*0.149, 0.539, -0.047),
                      0.100, 0.078, depth=0.91)
        actor.segment("lining", shin, (sign*0.151, 0.468, -0.013), (sign*0.165, 0.145, 0.009),
                      0.081, 0.061)
        actor.rings("armor", shin, [(0.151, sign*0.163, -0.011, 0.068, 0.074),
                                    (0.345, sign*0.158, -0.002, 0.094, 0.095),
                                    (0.426, sign*0.154, -0.019, 0.083, 0.089)])
        x = sign*0.151
        actor.plate("crest", shin, [(x-0.078, 0.473), (x, 0.526), (x+0.078, 0.473),
                                    (x+0.065, 0.428), (x, 0.402), (x-0.065, 0.428)],
                    -0.112, -0.082)
        actor.plate("shade", shin, [(x, 0.396), (x+0.023, 0.360), (x, 0.188), (x-0.023, 0.360)],
                    -0.093, -0.062)
        actor.rings("lining", foot, [(0.009, sign*0.166, -0.057, 0.087, 0.158),
                                     (0.051, sign*0.166, -0.057, 0.086, 0.157)])
        actor.rings("armor", foot, [(0.047, sign*0.166, -0.055, 0.085, 0.154),
                                    (0.103, sign*0.166, -0.026, 0.078, 0.132),
                                    (0.166, sign*0.166, 0.011, 0.062, 0.073)])
        actor.rings("crest", foot, [(0.050, sign*0.166, -0.147, 0.075, 0.062),
                                    (0.082, sign*0.166, -0.141, 0.072, 0.055)])

    actor.sword("hand.R", (0.444, 1.044, -0.300), (0.095, 0.79, -0.56),
                0.86 if enemy else 0.79, 0.121, heavy=enemy)
    actor.rings("shade", "hand.L", [(0.882, -0.438, -0.296, 0.08, 0.036),
                                    (1.002, -0.455, -0.306, 0.135, 0.037),
                                    (1.175, -0.470, -0.319, 0.11, 0.033)])
    actor.plate("armor", "hand.L", [(-0.549, 1.140), (-0.374, 1.140), (-0.345, 1.020),
                                    (-0.379, 0.928), (-0.444, 0.887), (-0.520, 0.931),
                                    (-0.564, 1.021)], -0.359, -0.312)
    actor.plate("brass", "hand.L", [(-0.452, 1.093), (-0.481, 1.045), (-0.449, 1.003),
                                    (-0.415, 1.048)], -0.384, -0.347)
    animate_knight(actor)
    return actor


def animate_knight(actor):
    actor.motion("idle", "pelvis", [(0, (0, 0.86, 0)), (0.85, (0, 0.876, 0)), (1.7, (0, 0.86, 0))],
                 "translation")
    actor.motion("idle", "spine", [(0, (2,-3,0)), (0.85, (-1, 2, 0.5)), (1.7, (2,-3,0))])
    actor.motion("idle", "head", [(0, (0,4,0)), (0.85, (-1, -2, 0)), (1.7, (0,4,0))])
    actor.motion("idle", "mantle", [(0, (5, 0, 0)), (0.85, (1, 3, -2)), (1.7, (5, 0, 0))])
    actor.motion("idle", "hand.R", [(0, (-8, -6, -8)), (0.85, (-2, -3, -5)), (1.7, (-8, -6, -8))])

    times = (0, 0.18, 0.36, 0.54, 0.72)
    for suffix, sign in (("L", -1), ("R", 1)):
        actor.motion("walk", "thigh."+suffix, list(zip(times, [
            (-22*sign, 0, 0), (0, 0, 0), (22*sign, 0, 0), (0, 0, 0), (-22*sign, 0, 0)])))
        actor.motion("walk", "shin."+suffix, list(zip(times, [
            (4, 0, 0), (20 if sign == 1 else 5, 0, 0), (6,0,0),
            (4 if sign == 1 else 20, 0, 0), (4, 0, 0)])))
        actor.motion("walk", "foot."+suffix, list(zip(times, [
            (4*sign, 0, 0), (6,0,0), (-4*sign, 0, 0), (6,0,0), (4*sign, 0, 0)])))
        actor.motion("walk", "arm."+suffix, list(zip(times, [
            (12*sign, 0, 0), (1,0,0), (-12*sign, 0, 0), (1,0,0), (12*sign, 0, 0)])))
    actor.motion("walk", "pelvis", list(zip(times, [
        (0,0.863,0), (0,0.829,0), (0,0.863,0), (0,0.829,0), (0,0.863,0)])), "translation")
    actor.motion("walk", "spine", list(zip(times, [(4, -7, -2), (3,0,0), (4,7,2), (3,0,0), (4,-7,-2)])))
    actor.motion("walk", "mantle", list(zip(times, [(12,3,0), (6,0,3), (12,-3,0), (6,0,-3), (12,3,0)])))

    # Wind-up, cross-body impact at 0.30 seconds, and deliberate recovery.
    times = (0, 0.14, 0.30, 0.48, 0.76)
    poses = {
        "spine": [(2,-3,0), (-9,-21,-5), (12,22,-7), (7,13,0), (2,-3,0)],
        "arm.R": [(0,0,0), (-37,-26,42), (-24,45,-17), (-9,21,-12), (0,0,0)],
        "elbow.R": [(0,0,0), (22,-32,8), (-14,22,10), (10,-4,2), (0,0,0)],
        "hand.R": [(-8,-6,-8), (-8,-12,16), (-25,12,-25), (-11,8,-11), (-8,-6,-8)],
        "arm.L": [(0,0,0), (-11,13,-10), (-14,-15,16), (-7,-8,10), (0,0,0)],
        "head": [(0,4,0), (3,15,0), (-2,-13,0), (0,-4,0), (0,4,0)],
        "mantle": [(5,0,0), (13,9,7), (31,-9,-13), (19,-13,-2), (5,0,0)],
    }
    for bone, rotations in poses.items():
        actor.motion("attack", bone, list(zip(times, rotations)))

    times = (0, 0.18, 0.49, 0.85, 1.15)
    actor.motion("defeat", "root", list(zip(times, [
        (0,0,0), (0,-0.02,0.03), (0,-0.22,0.16), (-0.10,0.28,0.49), (-0.13,0.295,0.50)])), "translation")
    actor.motion("defeat", "root", list(zip(times, [
        (0,0,0), (-13,4,-6), (-24,-10,-14), (-84,-17,-8), (-87,-17,-8)])))
    actor.motion("defeat", "head", list(zip(times, [
        (0,0,0), (-7,4,5), (21,12,-13), (14,-8,10), (17,-8,10)])))
    for suffix, sign in (("L", -1), ("R", 1)):
        actor.motion("defeat", "thigh."+suffix, list(zip(times, [
            (0,0,0), (4,0,0), (-29+sign*11,0,sign*9), (-11,0,sign*13), (-11,0,sign*13)])))
        actor.motion("defeat", "shin."+suffix, list(zip(times, [
            (0,0,0), (15,0,0), (50,0,0), (19,0,0), (17,0,0)])))
        actor.motion("defeat", "arm."+suffix, list(zip(times, [
            (0,0,0), (-18,-sign*13,sign*19), (-19,sign*9,sign*37), (0,sign*16,sign*51), (0,sign*15,sign*50)])))


def first_person():
    actor = Actor("First-person sword and gauntlets", PALETTE)
    actor.bone("view", None, (0, 0, 0))
    actor.bone("right forearm", "view", (0.50, -0.39, -0.33))
    actor.bone("right grip", "right forearm", (0.31, -0.248, -0.67))
    actor.bone("left forearm", "view", (-0.51, -0.43, -0.38))
    actor.bone("left grip", "left forearm", (-0.444, -0.355, -0.624))
    actor.segment("cloth", "right forearm", (0.65,-0.49,-0.23), (0.46,-0.363,-0.46), 0.132, 0.119, depth=0.79)
    actor.segment("shade", "right forearm", (0.48,-0.375,-0.405), (0.31,-0.248,-0.665), 0.106, 0.063)
    actor.segment("armor", "right forearm", (0.47,-0.362,-0.42), (0.328,-0.261,-0.633), 0.115, 0.075)
    for along, width in ((0.25,0.111), (0.51,0.099), (0.79,0.082)):
        outer, inner = (0.469,-0.362,-0.422), (0.314,-0.25,-0.655)
        first = add(outer, scale(sub(inner, outer), along))
        last = add(outer, scale(sub(inner, outer), along+0.10))
        actor.segment("crest", "right forearm", first, last, width, width*0.98)
    actor.box("lining", "right grip", (0.309,-0.248,-0.682), (0.067, 0.058, 0.062))
    for finger in (-0.039,-0.013,0.013,0.039):
        actor.box("crest", "right grip", (0.31+finger,-0.282,-0.706), (0.013,0.029,0.031))
    actor.box("armor", "right grip", (0.267,-0.226,-0.693), (0.021,0.042,0.047))
    actor.sword("right grip", (0.322,-0.233,-0.716), (0.205,0.60,-0.69), 0.74, 0.112)
    actor.segment("cloth", "left forearm", (-0.62,-0.52,-0.23), (-0.46,-0.382,-0.53), 0.109,0.083)
    actor.segment("armor", "left forearm", (-0.49,-0.425,-0.43), (-0.442,-0.363,-0.612), 0.094,0.068)
    actor.segment("crest", "left forearm", (-0.447,-0.377,-0.555), (-0.44,-0.370,-0.593), 0.077,0.072)
    actor.box("lining", "left grip", (-0.44,-0.344,-0.634), (0.055,0.050,0.060))
    actor.box("crest", "left grip", (-0.429,-0.326,-0.665), (0.050,0.035,0.042))
    actor.motion("idle", "view", [(0,(0,0,0)), (0.9,(-0.008,0.010,-0.009)), (1.8,(0,0,0))], "translation")
    actor.motion("idle", "right grip", [(0,(0,0,0)), (0.9,(2,-1,-2)), (1.8,(0,0,0))])
    times = (0, 0.16, 0.32, 0.48, 0.64)
    actor.motion("walk", "view", list(zip(times, [
        (0,0,0),(0.016,-0.026,-0.020),(0.0,0.015,-0.037),(-0.015,-0.025,-0.019),(0,0,0)])), "translation")
    actor.motion("walk", "right forearm", list(zip(times, [
        (0,0,0),(-3,0,2),(4,0,-2),(-3,0,2),(0,0,0)])))
    actor.motion("walk", "left forearm", list(zip(times, [
        (-5,0,0),(1,0,3),(-13,-3,1),(3,0,4),(-5,0,0)])))
    times = (0, 0.12, 0.21, 0.29, 0.39, 0.53, 0.67)
    actor.motion("attack", "view", list(zip(times, [
        (0,0,0),(0.094,0.044,0.040),(0.125,0.064,0.024),(-0.091,0.004,-0.14),
        (-0.154,-0.029,-0.071),(-0.045,-0.067,-0.035),(0,0,0)])), "translation")
    actor.motion("attack", "right forearm", list(zip(times, [
        (0,0,0),(-18,-17,-16),(-24,-24,-20),(-5,33,20),(16,52,24),(8,18,8),(0,0,0)])))
    actor.motion("attack", "right grip", list(zip(times, [
        (0,0,0),(-13,-13,-12),(-21,-15,-28),(8,21,37),(18,28,55),(6,9,19),(0,0,0)])))
    actor.motion("attack", "left forearm", list(zip(times, [
        (0,0,0),(-9,-6,3),(-10,-11,1),(-3,-8,0),(7,0,5),(3,0,4),(0,0,0)])))
    actor.motion("defeat", "view", [(0,(0,0,0)), (0.35,(-0.045,-0.25,0.09)),
                                    (0.85,(-0.11,-0.78,-0.17)), (1.15,(-0.11,-0.78,-0.17))], "translation")
    return actor


def main():
    ACTORS.mkdir(parents=True, exist_ok=True)
    files = [("knight.glb", armored_knight()), ("warden.glb", armored_knight(enemy=True)),
             ("first-person-sword.glb", first_person())]
    catalog = []
    for filename, model in files:
        data = Glb().serialize(model)
        (ACTORS / filename).write_bytes(data)
        path = f"actors/{filename}"
        catalog.append({
            "id": str(uuid.uuid5(uuid.NAMESPACE_URL, f"cinder-oath:{path}")),
            "path": path,
            "kind": "model",
            "revision": 1,
            "fingerprint": sha256(data).hexdigest(),
            "bytes": len(data),
            "meshes": [{"index": 0, "name": model.name, "primitives": len(model.geometry)}],
        })
    print(json.dumps(catalog, indent=2))


if __name__ == "__main__":
    main()
