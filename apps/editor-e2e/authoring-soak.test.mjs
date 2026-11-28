import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {appendFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {promisify} from 'node:util';
import test from 'node:test';
import {packageDirectory, withEditor} from './desktop.mjs';

const requestedSeconds = process.env.GAME_ENGINE_EDITOR_E2E_SOAK_SECONDS;
const seconds = Number(requestedSeconds ?? 0);
if (requestedSeconds && (!Number.isSafeInteger(seconds) || seconds < 10 || seconds > 21_600)) {
    throw new Error('GAME_ENGINE_EDITOR_E2E_SOAK_SECONDS must be between 10 and 21600 seconds');
}
const enabled = process.platform === 'win32' && !!packageDirectory && seconds > 0;
const sizes = [[760, 540], [1080, 720], [640, 540], [1440, 900]];
const state = 'window.__TAURI_INTERNALS__.invoke("editor_snapshot")';
const run = promisify(execFile);

async function openScene(view, sceneId, path) {
    const start = performance.now();
    await view.click('button[popovertarget="editor-game-menu"]');
    await view.click('#editor-game-menu button', {label: 'Scene Information…'});
    await view.fill('dialog[open] input[placeholder="Find a scene…"]', path);
    const option = '[aria-label="Scene results"] [role="option"][aria-selected="true"] > span';
    await view.waitFor('document.querySelector(' + JSON.stringify(option) + ')?.textContent === ' + JSON.stringify(path),
        'the exact registered scene');
    await view.key('Enter', {code: 'Enter', windowsVirtualKeyCode: 13});
    const revision = await view.waitFor(state + '.then(snapshot => snapshot.active_scene_id === ' +
        JSON.stringify(sceneId) + ' && snapshot.scene_revision)', 'the requested scene loaded');
    if (await view.evaluate('!!document.querySelector(\'dialog[open] input[placeholder="Find a scene…"]\')')) {
        await view.click('dialog[open] button', {label: 'Close Scene Information'});
    }
    await view.waitFor('!document.querySelector("dialog[open]")', 'Scene Information closed');
    try {
        await view.waitFor('window.__TAURI_INTERNALS__.invoke("editor_viewport_status").then(status =>' +
            ' status.visible && status.draw_calls > 0 && status.scene_load?.stage === "presented" &&' +
            ' status.scene_load.scene_id === ' + JSON.stringify(sceneId) +
            ' && status.scene_load.scene_revision === ' + revision + ')', 'the exact native scene presented');
    } catch (cause) {
        const observed = await Promise.allSettled([
            view.invoke('editor_viewport_status'),
            view.invoke('editor_viewport_editor_state'),
            view.evaluate('({ visibility: document.visibilityState, focused: document.hasFocus(),' +
                ' overlays: [...document.querySelectorAll("dialog[open], :popover-open")].map(el => el.nodeName),' +
                ' viewport: document.querySelector("scene-viewport .surface")?.getBoundingClientRect().toJSON() })'),
        ]);
        throw new Error('Scene presentation timed out for ' + sceneId + ' revision ' + revision + '\n' +
            JSON.stringify(observed), {cause});
    }
    return Math.round(performance.now() - start);
}

async function openSoundtrack(view) {
    await view.fill('input[aria-label="Search Assets"]', 'the-vault.wav');
    const row = '.asset-file[title="audio/the-vault.wav"] button';
    await view.waitFor('document.querySelectorAll(\'[aria-label="Asset file results"] li\').length === 1', 'a single soundtrack result');
    await view.click(row);
    await view.waitFor('document.querySelector(".asset-details .asset-path")?.textContent === "audio/the-vault.wav"', 'soundtrack details');
    await view.fill('input[aria-label="Search Assets"]', '');
}

async function previewKnight(view) {
    await view.fill('input[aria-label="Search Hierarchy"]', 'Knight of the Oath');
    await view.click('scene-entities-panel [role="treeitem"][data-key="oath-knight"]');
    await view.waitFor(state + '.then(snapshot => snapshot.selection[0] === "oath-knight")', 'the Knight is selected');
    await view.click('workspace-tabs [role="tab"]', {label: 'Animation'});
    await view.waitFor('document.querySelector(\'section[aria-label="Isolated Animation viewport"] .surface\')?.getClientRects().length',
        'the imported clip preview is ready');
    await view.waitFor('window.__TAURI_INTERNALS__.invoke("editor_viewport_status").then(status =>' +
        ' status.visible && status.draw_calls > 0)', 'the isolated animated object rendered');
    await view.click('workspace-tabs [role="tab"]', {label: 'Scene'});
    await view.fill('input[aria-label="Search Hierarchy"]', '');
    assert.equal((await view.invoke('editor_snapshot')).selection[0], 'oath-knight');
}

async function playAndReturn(view) {
    await view.click('workspace-toolbar button', {label: 'Play'});
    await view.waitFor(state + '.then(snapshot => snapshot.mode === "play")', 'Play started');
    await view.waitFor('document.querySelector(".game-status")?.textContent.includes("Running")', 'running Game view');
    await view.click('workspace-toolbar button', {label: 'Stop'});
    await view.waitFor(state + '.then(snapshot => snapshot.mode === "edit")', 'Play stopped');
    await view.waitFor('document.querySelector(\'[role="tab"][aria-selected="true"]\')?.textContent.trim() === "Scene"',
        'the authoring tab restored');
}

async function memory(view, pid) {
    const js = await view.send('Runtime.getHeapUsage');
    const result = await run('powershell.exe', [
        '-NoProfile', '-NonInteractive', '-Command', '(Get-Process -Id ' + Number(pid) + ').WorkingSet64',
    ], {timeout: 5000, windowsHide: true});
    const native = Number(result.stdout.trim());
    assert.ok(Number.isSafeInteger(native) && native > 0);
    return {nativeWorkingSetBytes: native, webViewJsHeapUsedBytes: js.usedSize};
}

test('Windows Editor keeps scene, asset, and Play state coherent across repeated authoring cycles', {
    skip: enabled ? false : 'Set the package, output root, and GAME_ENGINE_EDITOR_E2E_SOAK_SECONDS on Windows',
    timeout: (seconds + 120) * 1000,
}, () => withEditor('authoring-soak', async ({view, root, pid}) => {
    const original = await view.invoke('editor_snapshot');
    const startup = original.scenes.find(scene => scene.id === original.active_scene_id);
    const other = original.scenes.find(scene => scene.id !== original.active_scene_id);
    assert.ok(startup && other, 'Use the packaged multi-scene Cinder Oath test project');
    const report = join(root, 'cycles.jsonl');
    const reportSample = async sample => {
        await appendFile(report, JSON.stringify(sample) + '\n', 'utf8');
    };
    const started = Date.now();
    const deadline = started + seconds * 1000;
    let cycle = 0;
    const baseline = await memory(view, pid);
    await reportSample({type: 'start', time: new Date(started).toISOString(), seconds, ...baseline});
    console.log('Authoring soak evidence at ' + report);
    do {
        const cycleStart = Date.now();
        const [width, height] = sizes[cycle % sizes.length];
        await view.resize(width, height);
        const awayMs = await openScene(view, other.id, other.asset_path);
        const returnMs = await openScene(view, startup.id, startup.asset_path);
        await openSoundtrack(view);
        if (cycle % 5 === 0) await previewKnight(view);
        if (cycle % 3 === 0) await playAndReturn(view);
        const current = await view.invoke('editor_snapshot');
        assert.equal(current.active_scene_id, original.active_scene_id);
        assert.equal(current.mode, 'edit');
        assert.equal(current.transaction, null);
        assert.equal(current.dirty, false);
        assert.equal(current.project_dirty, false);
        assert.deepEqual(current.document, original.document);
        cycle += 1;
        const sample = {type: 'cycle', cycle, width, height, awayMs, returnMs};
        if (cycle === 1 || cycle % 10 === 0) {
            Object.assign(sample, await memory(view, pid));
            console.log('Authoring cycles verified ' + cycle);
        }
        await reportSample(sample);
        await delay(Math.max(0, Math.min(deadline, cycleStart + 12_000) - Date.now()));
    } while (Date.now() < deadline);
    const elapsedMs = Date.now() - started;
    await reportSample({type: 'completed', cycles: cycle, elapsedMs, ...await memory(view, pid)});
    console.log('Authoring soak completed ' + cycle + ' cycles in ' + Math.round(elapsedMs / 1000) + ' seconds');
}));

