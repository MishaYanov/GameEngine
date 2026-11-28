import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {mkdir, mkdtemp, readFile, readdir, realpath, writeFile} from 'node:fs/promises';
import {isAbsolute, join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {fileURLToPath} from 'node:url';
import {promisify} from 'node:util';
import test from 'node:test';
import {packageDirectory, withEditor} from './desktop.mjs';
import {assertNormalMapFrame, writeNormalMapFixture} from './native-normal-map-fixture.mjs';

const nativePointer = fileURLToPath(new URL('./native-pointer.py', import.meta.url));
const snapshotCall = 'window.__TAURI_INTERNALS__.invoke("editor_snapshot")';
const desktop = process.platform === 'win32' && !!packageDirectory;
const sdkLayer = process.env.GAME_ENGINE_EDITOR_E2E_VULKAN_LAYER_PATH;

test('Native viewport picks geometry, drags a visible X handle, and undoes the edit', {
    skip: desktop ? false : 'Set GAME_ENGINE_EDITOR_E2E_PACKAGE and GAME_ENGINE_EDITOR_E2E_ROOT on Windows',
    timeout: 100_000,
}, async () => {
    const frames = sdkLayer ? await createCaptureDirectory(sdkLayer) : null;
    const environment = frames ? {
        VK_LAYER_PATH: sdkLayer,
        VK_INSTANCE_LAYERS: 'VK_LAYER_LUNARG_screenshot',
        VK_SCREENSHOT_DIR: frames,
        VK_SCREENSHOT_FRAMES: '0-63',
    } : {};

    await withEditor('native-viewport', async ({view, root, pid}) => {
        assert.ok(pid, 'Expected the process launched by this fixture');
        await view.resize(1120, 780);
        const fixture = await createCubeFixture(view);
        const {front, rear} = fixture;
        const edit = command => editScene(view, command);
        const set = action => viewportAction(view, action);
        const setRear = (position, scale) => setTransform(view, rear, position, scale);

        await edit({type: 'select', keys: [front]});
        await view.waitFor('window.__TAURI_INTERNALS__.invoke("editor_viewport_status").then(status =>' +
            ' status.visible && status.draw_calls >= 2 && status.scene_load?.stage === "presented")',
            'both native mesh draws');
        await set({type: 'frame_selection'});
        await set({type: 'set_projection', projection: 'orthographic'});
        await set({type: 'orient', orientation: 'front'});
        await set({type: 'set_tool', tool: 'hand'});
        await set({type: 'set_grid', visible: false});
        await set({type: 'set_colliders', visible: false});
        await set({type: 'set_show_vertices', visible: false});
        await set({type: 'set_shading', shading: 'wireframe'});
        await edit({type: 'select', keys: []});
        await view.waitFor(snapshotCall + '.then(scene => scene.selection.length === 0)', 'cleared selection');

        const native = await pointer(pid, 'info');
        const {width, height, dpi} = native;
        const surface = await view.waitFor(
            'window.__TAURI_INTERNALS__.invoke("editor_viewport_status").then(status =>' +
            ' status.visible && status.width > 0 && status.height > 0 && status)',
            'the Vulkan backbuffer extent',
        );
        assert.ok(width > 0 && height > 0 && dpi > 0);

        if (frames) {
            await delay(250);
            const foreground = await captureNewest(frames, root, 'foreground-only', surface);
            await setRear([0, 0, -1.25], [0.7, 0.7, 0.7]);
            const covered = await captureNewest(frames, root, 'rear-fully-hidden', surface, foreground.index);
            assert.ok(foreground.data.equals(covered.data), 'A fully hidden mesh must not affect Wireframe pixels');
            await setRear([0.72, 0, -1.25], [0.7, 0.7, 0.7]);
            const partial = await captureNewest(frames, root, 'rear-partly-visible', surface, covered.index);
            const partlyVisible = externallyChangedPixels(foreground.data, partial.data, 'Rear mesh');
            await setRear([20, 0, -1.25], [0.7, 0.7, 0.7]);
            await set({type: 'set_plane', plane: 'xy'});
            await set({type: 'set_grid', visible: true});
            const grid = await captureNewest(frames, root, 'grid-behind-foreground', surface, partial.index);
            const gridPixels = externallyChangedPixels(foreground.data, grid.data, 'Grid', surface.width);
            assert.ok(gridPixels < surface.width * surface.height * 0.35, 'Grid toggle unexpectedly changes the entire viewport background');
            await writeFile(join(root, 'pixel-checks.json'), JSON.stringify({
                width: surface.width, height: surface.height, fullyCoveredChangedPixels: 0, partlyVisible, gridPixels,
            }, null, 2));
        }

        await set({type: 'set_grid', visible: false});
        await set({type: 'set_shading', shading: 'lit'});
        await set({type: 'set_tool', tool: 'move'});

        const x = Math.floor(width / 2);
        const y = Math.floor(height / 2);
        await pointer(pid, 'click', x, y);
        const key = JSON.stringify(front);
        const treeItem = 'scene-entities-panel [role="treeitem"][data-key=' + key + ']';
        await view.waitFor(snapshotCall + '.then(scene => scene.selection[0] === ' + key + ')', 'the mesh selected from the viewport');
        await view.waitFor('document.querySelector("inspector-panel .object-identity h3")?.textContent.trim() === "Foreground Cube"',
            'the viewport selection in Inspector');
        assert.equal(await view.evaluate('document.querySelector(' + JSON.stringify(treeItem) + ')?.getAttribute("aria-selected")'), 'true');

        const baseline = await view.invoke('editor_snapshot');
        const before = baseline.document.entities.find(entity => entity.key === front).transform;
        // Win32 client coordinates can differ from the Vulkan backbuffer on HiDPI.
        const xHandle = x + Math.round(54 * dpi / 96 * width / surface.width);
        await pointer(pid, 'drag', xHandle, y, xHandle + 44, y);
        const positionX = 'scene.document.entities.find(entity => entity.key === ' + key + ').transform.position[0]';
        await view.waitFor(snapshotCall + '.then(scene => ' + positionX + ' > ' + (before.position[0] + 0.05) + ')',
            'the committed X-axis drag');
        const after = (await view.invoke('editor_snapshot')).document.entities.find(entity => entity.key === front).transform;
        assert.deepEqual(after.position.slice(1), before.position.slice(1));
        assert.deepEqual(after.rotation, before.rotation);
        assert.deepEqual(after.scale, before.scale);

        await view.click('workspace-toolbar button', {label: 'Undo'});
        await view.waitFor(snapshotCall + '.then(scene => ' + positionX + ' === ' + before.position[0] + ')',
            'one Undo restoring the drag');
        assert.deepEqual((await view.invoke('editor_snapshot')).document, baseline.document);
        await writeFile(join(root, 'native-pointer-result.json'), JSON.stringify({
            native,
            surface,
            x,
            y,
            xHandle,
            before,
            after
        }, null, 2));
    }, {environment});
});

test('Native Scene right-click creates a primitive at the work-plane point and right-drag stays in the viewport', {
    skip: desktop ? false : 'Set GAME_ENGINE_EDITOR_E2E_PACKAGE and GAME_ENGINE_EDITOR_E2E_ROOT on Windows',
    timeout: 100_000,
}, () => withEditor('native-scene-context', async ({view, root, pid}) => {
    const statusCall = 'window.__TAURI_INTERNALS__.invoke("editor_viewport_status")';
    const rootMenu = '[role="menu"][aria-label="Workspace actions"]';
    const creationMenu = '[role="menu"][aria-label="Create Object"]';
    await view.resize(1120, 780);
    const fixture = await createCubeFixture(view);
    await editScene(view, {type: 'select', keys: [fixture.front]});
    const status = await view.waitFor(statusCall + '.then(status =>' +
        ' status.visible && status.draw_calls >= 2 && status.scene_load?.stage === "presented" && status)',
        'both native fixture meshes');
    await viewportAction(view, {type: 'frame_selection'});
    await viewportAction(view, {type: 'orient', orientation: 'top'});
    await viewportAction(view, {type: 'set_plane', plane: 'xz'});
    await viewportAction(view, {type: 'set_snap_increments', translation: 0.25, rotation_degrees: 15, scale: 0.1});
    await viewportAction(view, {type: 'set_snap', enabled: true});
    const native = await pointer(pid, 'info');
    const before = await view.invoke('editor_snapshot');
    // Use an upper-left client point where the menu can open without being clamped.
    const x = Math.round(native.width * 0.22);
    const y = Math.round(native.height * 0.24);
    const expected = await view.evaluate('(() => {const box = document.querySelector("scene-viewport .surface").getBoundingClientRect();' +
        ' return [box.left + box.width * ' + x / native.width +
        ', box.top + box.height * ' + y / native.height + '];})()');

    await view.evaluate('(() => {' +
        ' const bridge = window.__TAURI_INTERNALS__; window.nativeContextFixtures = [];' +
        ' const handler = bridge.transformCallback(event => window.nativeContextFixtures.push(event.payload));' +
        ' return bridge.invoke("plugin:event|listen", {event: "editor://viewport-context-menu", target: {kind: "Any"}, handler});' +
        ' })()');
    await pointer(pid, 'right-click', x, y);
    const menuContext = await view.waitFor('window.nativeContextFixtures[0]', 'the native right-click context');
    assert.equal(menuContext.scene_id, before.active_scene_id);
    assert.equal(menuContext.revision, before.revision);
    assert.ok(Number.isSafeInteger(menuContext.workspace_generation));
    assert.ok(Math.abs(menuContext.client_x - expected[0]) < 3);
    assert.ok(Math.abs(menuContext.client_y - expected[1]) < 3);
    const point = menuContext.position;
    assert.ok(Array.isArray(point) && point.length === 3 && point.every(Number.isFinite));
    assert.ok(Math.abs(point[1]) < 0.001 && Math.hypot(point[0], point[2]) >= 0.25);
    assert.ok(point.every(value => Math.abs(value * 4 - Math.round(value * 4)) < 0.001));

    await view.click(rootMenu + ' [role="menuitem"]', {label: 'Create Object'});
    await view.click(creationMenu + ' [role="menuitem"]', {label: 'Cube'});
    const after = await view.waitFor(snapshotCall + '.then(scene => scene.document.entities.length === ' +
        (before.document.entities.length + 1) + ' && scene)', 'the new primitive placed by the native menu');
    const placed = after.document.entities.find(entity => !before.document.entities.some(previous => previous.key === entity.key));
    assert.equal(placed.name, 'Cube');
    assert.deepEqual(placed.transform.position, point);
    await view.waitFor('!document.querySelector("[role=menu]")', 'the closed native context menu');
    await view.waitFor(statusCall + '.then(status => status.visible && status.width > 0)', 'the Scene viewport restored');

    const beforeDrag = await view.invoke('editor_viewport_editor_state');
    await pointer(pid, 'right-drag', x, y, x + Math.round(native.width * 0.15), y + 4);
    await delay(250);
    assert.equal(await view.evaluate('window.nativeContextFixtures.length'), 1, 'right-drag must not request another context menu');
    assert.equal(await view.evaluate('!!document.querySelector(' + JSON.stringify(rootMenu) + ')'), false);
    const afterDrag = await view.invoke('editor_viewport_editor_state');
    assert.equal(afterDrag.gesture, null);
    assert.equal(afterDrag.settings.view, beforeDrag.settings.view);
    await writeFile(join(root, 'native-scene-context.json'), JSON.stringify({
        native,
        status,
        expected,
        menuContext,
        placed
    }, null, 2));
}));

test('Native Play renders normal UVs, mirrored tangents, and reflected instances', {
    skip: desktop && sdkLayer ? false : 'Set the packaged Editor, output path and Vulkan screenshot layer on Windows',
    timeout: 100_000,
}, async () => {
    const frames = await createCaptureDirectory(sdkLayer);
    await withEditor('native-normal-maps', async ({view, root}) => {
        const manifestPath = await writeNormalMapFixture(join(root, 'normal-map-project'));
        await view.invoke('editor_open_project', {
            manifestPath, expectedRevision: null, discardUnsaved: false,
        });
        await view.evaluate('location.replace("/editor"); true');
        await view.waitFor('document.querySelector("editor-workspace")', 'the normal-map fixture workspace');
        await view.resize(1420, 840);
        const viewport = 'window.__TAURI_INTERNALS__.invoke("editor_viewport_status")';
        await view.waitFor(viewport + '.then(status => status.visible && status.draw_calls > 0)',
            'the fixture in Scene view');
        const previous = Math.max(-1, ...(await readdir(frames))
            .filter(file => /^\d+\.ppm$/.test(file)).map(file => Number.parseInt(file)));
        await view.click('workspace-toolbar button', {label: 'Play'});
        await view.waitFor(snapshotCall + '.then(s => s.mode === "play")', 'Play mode');
        await view.waitFor('window.__TAURI_INTERNALS__.invoke("editor_viewport_editor_state")' +
            '.then(s => s.settings.view === "game")', 'Game viewport selected');
        // Play can resize the child surface while the final toolbar settles.
        await delay(500);
        const surface = await view.waitFor(viewport + '.then(status => status.visible && ' +
            'status.width > 0 && status.height > 0 && status.draw_calls === 4 && status.instances === 6 && status)',
            'two instanced batches and two individual surfaces in Game view');
        await delay(400);
        const captured = await captureNewest(frames, root, 'normal-map-fixture', surface, previous);
        const samples = assertNormalMapFrame(captured.data, surface);
        await writeFile(join(root, 'normal-map-pixels.json'), JSON.stringify(samples, null, 2));
    }, {
        openSample: false, environment: {
            VK_LAYER_PATH: sdkLayer,
            VK_INSTANCE_LAYERS: 'VK_LAYER_LUNARG_screenshot',
            VK_SCREENSHOT_DIR: frames,
            VK_SCREENSHOT_FRAMES: '0-47',
        }
    });
});


async function createCubeFixture(view) {
    const path = 'scenes/viewport-occlusion.json';
    const {workspace_generation: generation, revision} = await view.invoke('editor_authoring_scope');
    assert.ok(Number.isSafeInteger(generation) && generation > 0);
    const created = await view.invoke('editor_create_scene', {
        path, expectedRevision: revision, expectedGeneration: generation,
    });
    const sceneId = created.scenes.find(scene => scene.asset_path === path).id;
    if (created.active_scene_id !== sceneId) await editScene(view, {type: 'switch_scene', scene_id: sceneId});
    const scene = await view.invoke('editor_snapshot');
    assert.equal(scene.active_scene_id, sceneId);
    await view.waitFor('window.__TAURI_INTERNALS__.invoke("editor_viewport_status").then(status => status.visible)', 'the native viewport');
    const {asset} = await view.invoke('editor_ensure_primitive', {
        kind: 'cube', expectedRevision: scene.revision, expectedGeneration: generation,
    });

    async function addCube(name, position) {
        const before = await view.invoke('editor_snapshot');
        const after = await view.invoke('editor_import_model', {
            path: asset.path,
            position,
            expectedRevision: before.revision,
            expectedGeneration: generation,
            expectedScene: sceneId,
        });
        const added = after.document.entities.find(entity => !before.document.entities.some(previous => previous.key === entity.key));
        assert.ok(added, 'Expected the imported cube');
        await editScene(view, {type: 'rename', key: added.key, name});
        return added.key;
    }

    const front = await addCube('Foreground Cube', [0, 0, 0.60]);
    const rear = await addCube('Rear Cube', [20, 0, -1.25]);
    return {front, rear};
}

async function editScene(view, command) {
    const {revision} = await view.invoke('editor_snapshot');
    return view.invoke('editor_execute_incremental', {request: {expected_revision: revision, command}});
}

async function setTransform(view, key, position, scale) {
    return editScene(view, {
        type: 'set_transforms', space: 'local', snap: false,
        edits: [{key, transform: {position, rotation: [0, 0, 0], scale}}]
    });
}

async function viewportAction(view, action) {
    const response = await view.evaluate('window.__TAURI_INTERNALS__.invoke("editor_viewport_action", ' +
        JSON.stringify({action}) + ').catch(error => ({qaError: String(error)}))');
    if (response.qaError) throw new Error(action.type + ' failed: ' + response.qaError);
    return response;
}

async function pointer(pid, action, ...positions) {
    const python = process.env.GAME_ENGINE_EDITOR_E2E_PYTHON || 'python';
    const {stdout} = await promisify(execFile)(python, [
        '-B', nativePointer, String(pid), join(packageDirectory, 'game-engine-editor.exe'),
        action, ...positions.map(String),
    ], {timeout: 5000, windowsHide: true});
    const target = JSON.parse(stdout);
    assert.equal(target.pid, pid);
    assert.equal(target.foreground_unchanged, true, 'Test pointer must not change desktop focus');
    return target;
}

async function createCaptureDirectory(layer) {
    const root = process.env.GAME_ENGINE_EDITOR_E2E_ROOT;
    assert.ok(root && isAbsolute(root) && isAbsolute(layer), 'Supply absolute output and Vulkan layer directories');
    await readFile(join(layer, 'VkLayer_screenshot.json'));
    await mkdir(root, {recursive: true});
    return mkdtemp(join(await realpath(root), 'vulkan-frames-'));
}

async function captureNewest(source, destination, label, {width, height}, previous = -1) {
    const deadline = Date.now() + 5000;
    while (Date.now() < deadline) {
        const indexes = (await readdir(source))
            .filter(file => /^\d+\.ppm$/.test(file))
            .map(file => Number.parseInt(file))
            .filter(index => index > previous)
            .sort((a, b) => b - a);
        for (const index of indexes) {
            const raw = await readFile(join(source, index + '.ppm'));
            const header = /^P6\s+(\d+)\s+(\d+)\s+255\r?\n/.exec(raw.toString('ascii', 0, 120));
            if (!header || Number(header[1]) !== width || Number(header[2]) !== height ||
                raw.length !== header[0].length + width * height * 3) continue;
            await delay(100);
            const newest = (await readdir(source)).filter(file => /^\d+\.ppm$/.test(file)).map(file => Number.parseInt(file));
            if (newest.some(frame => frame > index)) break;
            await writeFile(join(destination, label + '.ppm'), raw);
            return {index, data: raw.subarray(header[0].length)};
        }
        await delay(75);
    }
    throw new Error('Did not capture a fresh ' + width + '×' + height + ' Vulkan frame for ' + label);
}

function externallyChangedPixels(baseline, next, overlay, boundaryWidth) {
    assert.equal(next.length, baseline.length);
    const histogram = new Map();
    for (let index = 0; index < baseline.length; index += 3) {
        const color = baseline.readUIntBE(index, 3);
        histogram.set(color, (histogram.get(color) ?? 0) + 1);
    }
    const [backdrop] = [...histogram.entries()].sort((a, b) => b[1] - a[1])[0];
    let foreground = 0;
    let changed = 0;
    for (let index = 0; index < baseline.length; index += 3) {
        const before = baseline.readUIntBE(index, 3);
        const after = next.readUIntBE(index, 3);
        if (before !== backdrop) {
            foreground++;
            if (before !== after) {
                // A one-pixel wire edge can extend outside the filled surface at the silhouette.
                // Grid can blend at that boundary; interior mesh pixels must be identical.
                const silhouette = boundaryWidth && isSilhouettePixel(baseline, index / 3, boundaryWidth, backdrop);
                assert.ok(silhouette, overlay + ' changed an interior mesh pixel at index ' + index / 3);
            }
        } else if (before !== after) changed++;
    }
    assert.ok(foreground > 1000 && changed > 1000, overlay + ' needs distinct foreground and exterior pixels');
    return changed;
}

function isSilhouettePixel(pixels, index, width, backdrop) {
    const row = Math.floor(index / width);
    const column = index % width;
    const height = pixels.length / (width * 3);
    const neighbors = [
        column > 0 ? index - 1 : null,
        column < width - 1 ? index + 1 : null,
        row > 0 ? index - width : null,
        row < height - 1 ? index + width : null,
    ];
    return neighbors.some(neighbor => neighbor !== null && pixels.readUIntBE(neighbor * 3, 3) === backdrop);
}
