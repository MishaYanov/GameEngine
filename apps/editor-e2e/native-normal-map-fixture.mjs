// A synthetic glTF fixture with opposing normal texels and one-sided geometry.
import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
import {join} from 'node:path';

const baseColorPng =
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGPo6Oj4DwAFzAKYMkOunAAAAABJRU5ErkJggg==';
const normalPng =
    'iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR4nGN81nDrvzEDAwMAFaUDdO8fj/cAAAAASUVORK5CYII=';
const fieldOfView = .88;
const cameraHeight = 1.2;
const cameraDistance = 11;
const panels = [
    ['flat', 'flat', -4.3, 2, 1],
    ['mapped', 'mapped', -1.45, 2, 1],
    ['reflected', 'mapped', 1.45, 2, -1],
    ['mirroredUv', 'mirrored', 4.3, 2, 1],
    ['mappedInstance', 'mapped', -1.45, .25, 1],
    ['reflectedInstance', 'mapped', 1.45, .25, -1],
];

export async function writeNormalMapFixture(root) {
    const assets = join(root, 'assets');
    await mkdir(join(assets, 'models'), {recursive: true});
    await mkdir(join(assets, 'scenes'), {recursive: true});
    await Promise.all(['flat', 'mapped', 'mirrored'].map(kind =>
        writeFile(join(assets, 'models', kind + '.glb'), makePanel(kind)),
    ));
    const scenePath = 'scenes/normal.json';
    await writeFile(join(assets, scenePath), JSON.stringify({
        schema_version: 7,
        bindings: {},
        organization: {folders: [], root_membership: {}},
        camera: {
            position: [0, cameraHeight, cameraDistance], pitch: 0, yaw: 0,
            vertical_fov: fieldOfView, near: .1, far: 100,
        },
        environment: {ambient_linear: [.095, .095, .095], sky_exposure_ev: 0},
        lights: [{
            key: 'sliding-light', name: 'Sliding Light', position: [0, 0, 0], kind: 'directional',
            direction: [-.8, 0, -.6], color: [1, 1, 1], intensity: .85, range: 20,
        }],
        entities: panels.map(([key, model, x, y, sign]) => ({
            key, name: key, model_path: 'models/' + model + '.glb', mesh_index: 0,
            active: true, renderable: true, collision_enabled: false,
            transform: {position: [x, y, 0], rotation: [0, 0, 0], scale: [sign * 1.03, .77, 1.29]},
        })),
    }));
    const manifestPath = join(root, 'project.json');
    await writeFile(manifestPath, JSON.stringify({
        schema_version: 3, name: 'Normal Map Check', fixed_hz: 60, max_catchup_steps: 8, behaviors: [],
        startup_scene_id: 'normal', scenes: [{id: 'normal', asset_path: scenePath}],
    }));
    return manifestPath;
}

export function assertNormalMapFrame(pixels, {width, height}) {
    const focalScale = height / (2 * Math.tan(fieldOfView / 2) * cameraDistance);
    const values = Object.fromEntries(panels.map(([key, , x, y]) => {
        const column = Math.round(width / 2 + x * focalScale);
        const row = Math.round(height / 2 - (y - cameraHeight) * focalScale);
        let total = 0;
        for (let dy = -6; dy <= 6; dy++) {
            for (let dx = -6; dx <= 6; dx++) {
                const byte = ((row + dy) * width + column + dx) * 3;
                total += (pixels[byte] + pixels[byte + 1] + pixels[byte + 2]) / 3;
            }
        }
        return [key, total / 169];
    }));
    const {flat, mapped, reflected, mirroredUv, mappedInstance, reflectedInstance} = values;
    const evidence = JSON.stringify(values);
    assert.ok(flat > 85 && flat < 125, 'Flat reference must be visible. ' + evidence);
    assert.ok(mapped > flat + 13, 'The image sampled through the normal UV must tilt into the light. ' + evidence);
    assert.ok(reflected > 20 && reflected < flat - 35,
        'The reflected geometry must be visible with its normal tilted away from the light. ' + evidence);
    assert.ok(Math.abs(mirroredUv - reflected) < 5, 'Mirrored UV handedness must match reflection. ' + evidence);
    assert.ok(Math.abs(mappedInstance - mapped) < 5 && Math.abs(reflectedInstance - reflected) < 5,
        'Instanced copies must keep the same normal response. ' + evidence);
    return values;
}

function makePanel(kind) {
    const data = [];
    const bufferViews = [];
    const accessors = [];
    let length = 0;

    function view(bytes, target) {
        const padding = (4 - length % 4) % 4;
        data.push(Buffer.alloc(padding), bytes);
        length += padding;
        bufferViews.push({
            buffer: 0, byteOffset: length, byteLength: bytes.length, ...(target ? {target} : {}),
        });
        length += bytes.length;
        return bufferViews.length - 1;
    }

    function attribute(values, type, components, bounds = {}) {
        const bytes = Buffer.alloc(values.length * 4);
        values.forEach((value, i) => bytes.writeFloatLE(value, i * 4));
        accessors.push({
            bufferView: view(bytes, 34962), componentType: 5126, count: values.length / components, type, ...bounds,
        });
        return accessors.length - 1;
    }

    const reflected = kind === 'mirrored';
    const attributes = {
        POSITION: attribute([-1, -1, 0, 1, -1, 0, 1, 1, 0, -1, 1, 0], 'VEC3', 3,
            {min: [-1, -1, 0], max: [1, 1, 0]}),
        NORMAL: attribute([0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1], 'VEC3', 3),
        TEXCOORD_0: attribute([.68, .8, .82, .8, .82, .2, .68, .2], 'VEC2', 2),
        TEXCOORD_1: attribute((reflected ? [.32, .8, .18, .8, .18, .2, .32, .2] :
            [.18, .8, .32, .8, .32, .2, .18, .2]), 'VEC2', 2),
    };
    const tangent = reflected ? [-1, 0, 0, 1] : [1, 0, 0, -1];
    attributes.TANGENT = attribute([...tangent, ...tangent, ...tangent, ...tangent], 'VEC4', 4);

    // Engine's existing positive front-facing asset winding.
    const indices = Buffer.from([0, 0, 2, 0, 1, 0, 2, 0, 0, 0, 3, 0]);
    const indexAccessor = accessors.push({
        bufferView: view(indices, 34963), componentType: 5123, count: 6, type: 'SCALAR',
    }) - 1;
    const images = [baseColorPng, normalPng].map(png => ({
        bufferView: view(Buffer.from(png, 'base64')), mimeType: 'image/png',
    }));
    const material = {
        doubleSided: false,
        pbrMetallicRoughness: {baseColorTexture: {index: 0}, metallicFactor: 0, roughnessFactor: 1},
    };
    if (kind !== 'flat') material.normalTexture = {index: 1, texCoord: 1, scale: 1};

    const scene = Buffer.from(JSON.stringify({
        asset: {version: '2.0'}, scene: 0, scenes: [{nodes: [0]}], nodes: [{mesh: 0}],
        buffers: [{byteLength: length}], bufferViews, accessors,
        meshes: [{primitives: [{attributes, indices: indexAccessor, material: 0}]}],
        images, materials: [material], textures: [{source: 0}, {source: 1}],
    }));
    const json = Buffer.concat([scene, Buffer.alloc((4 - scene.length % 4) % 4, 0x20)]);
    const bin = Buffer.concat([...data, Buffer.alloc((4 - length % 4) % 4)]);
    const header = Buffer.alloc(12);
    header.write('glTF');
    header.writeUInt32LE(2, 4);
    header.writeUInt32LE(28 + json.length + bin.length, 8);
    const chunk = (kind, bytes) => {
        const size = Buffer.alloc(4);
        size.writeUInt32LE(bytes.length);
        return [size, Buffer.from(kind), bytes];
    };
    return Buffer.concat([header, ...chunk('JSON', json), ...chunk('BIN\0', bin)]);
}
