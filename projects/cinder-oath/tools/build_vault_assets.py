"""Build the original room primitive and synthesized Cinder Oath audio."""

from array import array
import json
from math import exp, pi, sin
from pathlib import Path
from random import Random
from struct import pack
import sys
import wave

ASSETS = Path(__file__).resolve().parents[1] / "assets"
RATE = 16_000


def write_stone_block(destination):
    # Each face owns its vertices for crisp normals at the corners.
    faces = (
        ((1, 0, 0), (0, 0, -1), (0, 1, 0)),
        ((-1, 0, 0), (0, 0, 1), (0, 1, 0)),
        ((0, 1, 0), (1, 0, 0), (0, 0, -1)),
        ((0, -1, 0), (1, 0, 0), (0, 0, 1)),
        ((0, 0, 1), (1, 0, 0), (0, 1, 0)),
        ((0, 0, -1), (-1, 0, 0), (0, 1, 0)),
    )
    positions, normals, indices = [], [], []
    for normal, right, up in faces:
        base = len(positions) // 3
        for x, y in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
            positions.extend((n + x * r + y * u) / 2 for n, r, u in zip(normal, right, up))
            normals.extend(normal)
        indices.extend(base + value for value in (0, 1, 2, 0, 2, 3))
    buffers = (
        pack("<72f", *positions), pack("<72f", *normals), pack("<36H", *indices),
    )
    views = []
    offset = 0
    for index, chunk in enumerate(buffers):
        views.append({
            "buffer": 0, "byteOffset": offset, "byteLength": len(chunk),
            "target": 34963 if index == 2 else 34962,
        })
        offset += len(chunk)
    file = {
        "asset": {"version": "2.0", "generator": "Cinder Oath vault assets"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": "Vault masonry", "mesh": 0}],
        "meshes": [{"name": "Vault masonry", "primitives": [{
            "attributes": {"POSITION": 0, "NORMAL": 1}, "indices": 2, "material": 0,
        }]}],
        "materials": [{"pbrMetallicRoughness": {"metallicFactor": 0.0, "roughnessFactor": 0.88}}],
        "buffers": [{"byteLength": offset}],
        "bufferViews": views,
        "accessors": [
            {"bufferView": 0, "count": 24, "componentType": 5126, "type": "VEC3",
             "min": [-0.5] * 3, "max": [0.5] * 3},
            {"bufferView": 1, "count": 24, "componentType": 5126, "type": "VEC3"},
            {"bufferView": 2, "count": 36, "componentType": 5123, "type": "SCALAR"},
        ],
    }
    encoded = json.dumps(file, separators=(",", ":")).encode()
    encoded += b" " * (-len(encoded) % 4)
    binary = b"".join(buffers)
    payload = pack("<I4s", len(encoded), b"JSON") + encoded
    payload += pack("<I4s", len(binary), b"BIN\0") + binary
    destination.write_bytes(pack("<4sII", b"glTF", 2, 12 + len(payload)) + payload)


def frequency(note):
    return 440 * 2 ** ((note - 69) / 12)


def pulse(phase, duty=0.25):
    return 1.0 if phase % 1.0 < duty else -1.0


def envelope(t, duration):
    return min(1.0, t / 0.006, (duration - t) / 0.025) * exp(-1.4 * t)


def soundtrack():
    subdivision = 60 / 108 / 4
    motif = (
        (62, 69, 72, None, 74, 72, 69, None),
        (65, 69, 77, None, 76, 72, 69, None),
        (62, 69, 72, 74, 77, None, 76, 72),
        (64, 68, 71, None, 74, 71, 68, None),
        (62, 74, 77, None, 81, 77, 74, 72),
        (65, 72, 76, None, 77, 76, 72, 69),
        (67, 74, 77, 79, 82, None, 79, 77),
        (64, 68, 71, 74, 71, 68, 64, None),
    )
    roots = (38, 41, 38, 33, 38, 41, 43, 33)
    noise = Random(19)
    for sample in range(round(128 * subdivision * RATE)):
        time = sample / RATE
        step = min(127, int(time / subdivision))
        bar, index = divmod(step, 16)
        beat = (time - step * subdivision) + (index % 4) * subdivision
        note = motif[bar][index // 2]
        melody_age = time - (step // 2 * 2) * subdivision
        voice = 0.0
        if note is not None:
            voice += 0.23 * pulse(frequency(note) * melody_age) * envelope(melody_age, subdivision * 2)
        bass_note = roots[bar] + (7 if index % 8 >= 4 else 0)
        triangle = 4 * abs((frequency(bass_note) * beat) % 1.0 - 0.5) - 1
        voice += 0.29 * triangle * envelope(beat, subdivision * 4)
        if index % 8 < 2:
            voice += 0.12 * sin(2 * pi * (47 * beat + 1 - exp(-35 * beat))) * exp(-22 * beat)
        if index % 4 == 2:
            tick = time - step * subdivision
            voice += 0.04 * noise.uniform(-1, 1) * exp(-60 * tick)
        edge = min(1, time / 0.012, (128 * subdivision - time) / 0.05)
        yield round(voice * 8) / 8 * edge * 0.66


def effect(kind):
    times = {"swing": .22, "hit": .26, "hurt": .30, "victory": .82, "defeat": .75}
    duration = times[kind]
    noise = Random(240)
    for sample in range(round(RATE * duration)):
        t = sample / RATE
        amp = envelope(t, duration)
        if kind == "swing":
            amp *= (t / duration) ** .45 * exp(-5 * t / duration)
            value = noise.uniform(-1, 1) + 0.14 * sin(2 * pi * (240 * t - 210 * t * t))
        elif kind in ("hit", "hurt"):
            pitch = 431 if kind == "hit" else 104
            value = (
                .40 * sin(2 * pi * pitch * t)
                + .2 * sin(2 * pi * pitch * 1.48 * t)
                + .26 * exp(-35 * t) * noise.uniform(-1, 1)
            ) * exp(-12 * t)
        else:
            notes = (62, 69, 74, 77) if kind == "victory" else (57, 53, 50, 45)
            segment = duration / len(notes)
            index = min(len(notes) - 1, int(t / segment))
            age = t - index * segment
            value = .43 * pulse(age * frequency(notes[index]), .4) * envelope(age, segment)
        yield value * amp * .62


def write_wave(destination, samples):
    pcm = array("h", (round(max(-1, min(1, value)) * 32767) for value in samples))
    if sys.byteorder != "little":
        pcm.byteswap()
    with wave.open(str(destination), "wb") as output:
        output.setparams((1, 2, RATE, 0, "NONE", "not compressed"))
        output.writeframes(pcm.tobytes())


def main():
    masonry = ASSETS / "architecture"
    audio = ASSETS / "audio"
    masonry.mkdir(parents=True, exist_ok=True)
    audio.mkdir(parents=True, exist_ok=True)
    write_stone_block(masonry / "masonry.glb")
    write_wave(audio / "the-vault.wav", soundtrack())
    for name in ("swing", "hit", "hurt", "victory", "defeat"):
        write_wave(audio / f"blade-{name}.wav", effect(name))


if __name__ == "__main__":
    main()
