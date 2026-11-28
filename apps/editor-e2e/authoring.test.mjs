import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdir, readFile} from 'node:fs/promises';
import {join} from 'node:path';
import test from 'node:test';
import {packageDirectory, withEditor} from './desktop.mjs';

const available = process.platform === 'win32' && !!packageDirectory;
const desktop = {
    skip: available ? false : 'Set GAME_ENGINE_EDITOR_E2E_PACKAGE and GAME_ENGINE_EDITOR_E2E_ROOT on Windows',
    timeout: 100_000
};

test('New Scene creates, opens and saves a blank scene', desktop, () =>
    withEditor('new-scene', async ({view, project, root}) => {
        await view.resize(1080, 720);
        const original = await view.invoke('editor_snapshot');
        await view.click('button[popovertarget="editor-game-menu"]');
        await view.click('#editor-game-menu button', {label: 'New Scene…'});
        await view.click('#new-scene-name');
        await view.type('Acceptance Scene');
        const path = await view.waitFor('document.querySelector("#new-scene-path strong")?.innerText', 'an accepted scene name');
        await view.click('dialog[open] button', {label: 'Create Scene'});
        const sceneId = JSON.stringify(original.active_scene_id);
        await view.waitFor('window.__TAURI_INTERNALS__.invoke("editor_snapshot").then(value => value.active_scene_id !== ' + sceneId + ')', 'the blank scene');
        const opened = await view.invoke('editor_snapshot');
        assert.equal(opened.scenes.find(scene => scene.id === opened.active_scene_id)?.asset_path, path);
        assert.equal(opened.document.entities.length, 0);
        await view.click('workspace-toolbar button', {label: 'Save All'});
        await view.waitFor('window.__TAURI_INTERNALS__.invoke("editor_snapshot").then(value => !value.project_dirty)', 'a saved scene catalog');
        const manifest = JSON.parse(await readFile(join(project, 'project.json'), 'utf8'));
        assert.ok(manifest.scenes.some(scene => scene.asset_path === path));
        await view.capture(join(root, 'blank-scene.png'));
    }));

test('Assets and workspace actions fit a narrow Editor window', desktop, () =>
    withEditor('narrow-assets', async ({view, root}) => {
        await view.resize(760, 540);
        await view.click('editor-assets-panel button[data-asset-kind="scene"]');
        await view.waitFor('document.querySelector("editor-assets-panel button[data-asset-kind=scene]").getAttribute("aria-pressed") === "true"', 'the active Scenes filter');
        assert.equal(await view.evaluate('document.querySelectorAll(".asset-browser select").length'), 0);
        assert.equal(await view.evaluate('(() => {const browser = document.querySelector(".asset-browser").getBoundingClientRect(); return [...document.querySelectorAll(".asset-filters button")].every(button => button.getBoundingClientRect().right <= browser.right + 1);})()'), true);
        const height = await view.evaluate('document.querySelector(".workspace-panel").getBoundingClientRect().height');
        assert.ok(height >= 60, 'The viewport needs visible space below its controls');
        const point = await view.waitFor(`(() => {
            const chrome = document.querySelector('.workspace-controls');
            const rect = chrome?.getBoundingClientRect();
            if (!rect) return null;
            for (let x = rect.right - 8; x > rect.left + 7; x -= 8) {
                for (let y = rect.bottom - 6; y > rect.top + 5; y -= 4) {
                    if (document.elementFromPoint(x, y) === chrome) return {x, y};
                }
            }
            return null;
        })()`, 'empty workspace chrome');
        await view.send('Input.dispatchMouseEvent', {type: 'mouseMoved', ...point});
        await view.send('Input.dispatchMouseEvent', {type: 'mousePressed', button: 'right', clickCount: 1, ...point});
        await view.send('Input.dispatchMouseEvent', {type: 'mouseReleased', button: 'right', clickCount: 1, ...point});
        await view.waitFor('[...document.querySelectorAll("[role=menuitem]")].some(item => item.textContent.includes("Build Game"))', 'the workspace project menu');
        assert.equal(await view.evaluate('(() => {const panel=document.querySelector(".menu-panel").getBoundingClientRect(); return panel.left >= 0 && panel.top >= 0 && panel.right <= innerWidth && panel.bottom <= innerHeight;})()'), true);
        await view.key('Escape', {code: 'Escape'});
        await view.waitFor('!document.querySelector("[role=menuitem]")', 'the menu closing');
        await view.capture(join(root, 'narrow-assets.png'));
    }));


const hierarchy = 'scene-entities-panel [role="treeitem"]';
const knightName = 'Knight of the Oath';
const newObjectButton = 'scene-entities-panel button[aria-label="Create Object"]';
const snapshotExpression = 'window.__TAURI_INTERNALS__.invoke("editor_snapshot")';

function predicate(source, code) {
    return snapshotExpression + '.then(' + source + ' => ' + code + ')';
}

function textAt(selector, value) {
    return 'document.querySelector(' + JSON.stringify(selector) + ')?.textContent.trim() === ' + JSON.stringify(value);
}

async function chooseKnight(view) {
    const snapshot = await view.invoke('editor_snapshot');
    const knight = snapshot.document.entities.find(entity => entity.name === knightName);
    assert.ok(knight);
    await view.click(hierarchy + '[data-key=' + JSON.stringify(knight.key) + ']');
    await view.waitFor(textAt('inspector-panel .object-identity h3', knightName), 'Knight in the Inspector');
    return knight.key;
}

async function createCube(view) {
    await view.click(newObjectButton);
    await view.waitFor('window.__TAURI_INTERNALS__.invoke("editor_viewport_status").then(status =>' +
        ' !status.visible && !!document.querySelector(\'.menu-layer:popover-open [aria-label="Create scene object"]\'))',
        'the Create Object menu above the native viewport');
    await view.click('[role="menuitem"]', {label: 'Cube'});
    await view.waitFor(textAt('inspector-panel .object-identity h3', 'Cube'), 'Cube in the Inspector');
    const scene = await view.invoke('editor_snapshot');
    const cube = scene.document.entities.find(entity => scene.selection.includes(entity.key));
    assert.equal(cube?.name, 'Cube');
    return cube.key;
}

async function makeChildOfKnight(view) {
    const opener = 'scene-entities-panel button[aria-label="Make Child Of…"]';
    const dialog = 'dialog[open]:has(.parent-picker)';
    const picker = dialog + ' input[role="combobox"]';
    await view.click(opener);
    await view.waitFor('document.activeElement === document.querySelector(' + JSON.stringify(picker) + ')',
        'focus in Parent object');
    await view.type(knightName);
    await view.waitFor(textAt(dialog + ' [role="option"][aria-selected="true"] > span', knightName),
        'the offscreen parent in search results');
    const beforeApply = (await view.invoke('editor_snapshot')).revision;
    await view.key('Enter', {code: 'Enter', windowsVirtualKeyCode: 13});
    assert.equal((await view.invoke('editor_snapshot')).revision, beforeApply);
    await view.click(dialog + ' button', {label: 'Apply Parent'});
    await view.waitFor('!document.querySelector(' + JSON.stringify(dialog) + ')', 'the parent picker closed');
}

test('Hierarchy and Inspector track selection, creation, rename, Undo and parenting', desktop, () =>
    withEditor('object-edit', async ({view, root}) => {
        await view.resize(760, 540);
        const knight = await chooseKnight(view);
        const cube = await createCube(view);
        const cubeRow = hierarchy + '[data-key=' + JSON.stringify(cube) + ']';
        assert.equal(await view.evaluate('document.querySelector(' + JSON.stringify(cubeRow) + ')?.getAttribute("aria-selected")'), 'true');

        const name = 'Acceptance Block with a Longer Name';
        await view.fill('inspector-panel .name-field input', name);
        await view.key('Enter', {code: 'Enter', windowsVirtualKeyCode: 13});
        await view.waitFor(textAt(cubeRow + ' .entity-name', name), 'the changed name in the Hierarchy');
        const revision = (await view.invoke('editor_snapshot')).revision;
        await view.click('workspace-toolbar button', {label: 'Undo'});
        await view.waitFor(predicate('scene', 'scene.revision > ' + revision +
            ' && scene.document.entities.some(entity => entity.key === ' + JSON.stringify(cube) +
            ' && entity.name === "Cube")'), 'the undone rename');
        assert.deepEqual((await view.invoke('editor_snapshot')).selection, [cube]);

        await makeChildOfKnight(view);
        await view.waitFor(predicate('scene', 'scene.document.entities.some(entity => entity.key === ' +
            JSON.stringify(cube) + ' && entity.parent === ' + JSON.stringify(knight) + ')'), 'Cube parented to Knight');
        await view.waitFor(textAt('inspector-panel .object-identity h3', 'Cube'), 'the same selected object after parenting');
        await view.capture(join(root, 'parented-cube.png'));
    }));

test('Project assets expose the game soundtrack during Edit and Play', desktop, () =>
    withEditor('game-assets', async ({view, root}) => {
        await view.resize(760, 540);
        await view.click('editor-assets-panel button', {label: 'Entire project'});
        await view.fill('input[aria-label="Search Assets"]', 'the-vault.wav');
        const soundtrack = 'editor-assets-panel li[title="audio/the-vault.wav"] button';
        await view.waitFor('document.querySelectorAll(\'[aria-label="Asset file results"] li\').length === 1',
            'one soundtrack result');
        await view.click(soundtrack);
        await view.waitFor(textAt('.asset-details .asset-path', 'audio/the-vault.wav'), 'the selected soundtrack file');
        await view.click('workspace-toolbar button', {label: 'Play'});
        await view.waitFor('document.querySelector(".game-status")?.textContent.includes("Running")', 'live Game Preview');
        const before = await view.invoke('editor_snapshot');
        assert.equal(before.mode, 'play');
        await view.fill('input[aria-label="Search Assets"]', 'blade-hit.wav');
        const effect = 'editor-assets-panel li[title="audio/blade-hit.wav"] button';
        await view.waitFor('document.querySelector(\'[aria-label="Asset file results"] li\')?.title === "audio/blade-hit.wav"',
            'sound effect results while playing');
        await view.click(effect);
        await view.waitFor(textAt('.asset-details .asset-path', 'audio/blade-hit.wav'), 'the selected sound effect file');
        await view.capture(join(root, 'soundtrack-in-play.png'));
        await view.click('workspace-toolbar button', {label: 'Stop'});
        await view.waitFor(predicate('scene', 'scene.mode === "edit"'), 'return to Edit');
    }));

test('An existing visual clip reopens by identity from Animation and Assets', desktop, () =>
    withEditor('saved-clip', async ({view, project, root}) => {
        await view.resize(1080, 720);
        const knight = await chooseKnight(view);
        const cube = await createCube(view);
        await makeChildOfKnight(view);
        await view.waitFor(predicate('scene', 'scene.document.entities.some(entity => entity.key === ' +
            JSON.stringify(cube) + ' && entity.parent === ' + JSON.stringify(knight) + ')'), 'the animatable child');
        await view.click('workspace-toolbar button', {label: 'Save All'});
        await view.waitFor(predicate('scene', '!scene.dirty && !scene.project_dirty'), 'saved scene and catalog');
        await view.click('workspace-tabs [role="tab"]', {label: 'Animation'});
        await view.click('editor-animation-workspace button', {label: 'Authored Clips'});
        await view.click('editor-authored-clip-editor button', {label: 'New Visual Clip'});
        const clipName = 'Acceptance Turn in the Entrance Chamber';
        await view.fill('editor-authored-clip-editor input[maxlength="128"]', clipName);
        const path = await view.waitFor('document.querySelector(\'editor-authored-clip-editor input[maxlength="1024"]\')?.value',
            'the generated clip file path');
        assert.match(path, /\.clip\.json$/);
        await view.click('editor-authored-clip-editor button', {label: 'Save Clip'});
        await view.waitFor(textAt('.clip-file', path), 'saved clip path');
        const clipFile = join(project, 'assets', path);
        const contents = await readFile(clipFile, 'utf8');

        await view.click('workspace-tabs [role="tab"]', {label: 'Scene'});
        await view.click('workspace-tabs [role="tab"]', {label: 'Animation'});
        const animationHeading = '[aria-label="Clip timeline"] h3';
        const heading = clipName + ' Timeline';
        const picker = 'editor-search-picker:has(label) input[placeholder="Find a saved clip…"]';
        await view.click(picker);
        await view.waitFor(textAt('[role="option"][aria-selected="true"] > span', path), 'the original clip result');
        await view.key('Enter', {code: 'Enter', windowsVirtualKeyCode: 13});
        await view.waitFor(textAt(animationHeading, heading), 'existing Clip Timeline');
        assert.equal(await view.evaluate(textAt('.clip-file', path)), true);
        assert.equal(await view.evaluate('!!document.querySelector(\'input[placeholder="Find a visible child object…"]\')'), false);
        assert.equal(await readFile(clipFile, 'utf8'), contents);
        const openClip = await view.invoke('editor_animation_clips');
        const original = openClip.find(asset => asset.path === path);
        assert.ok(original?.asset_id);

        const bookmark = 'Approach confirmed';
        await view.fill('[aria-label="Timeline bookmarks"] input[maxlength="128"]', bookmark);
        await view.click('[aria-label="Timeline bookmarks"] button', {label: 'Add Bookmark'});
        await view.waitFor('document.querySelector("[aria-label=\'Clip timeline\'] header")?.textContent.includes("Unsaved changes")',
            'a drafted Timeline edit');
        assert.equal(await readFile(clipFile, 'utf8'), contents);
        await view.click('[aria-label="Clip timeline"] button', {label: 'Save Timeline'});
        await view.waitFor('!document.querySelector("[aria-label=\'Clip timeline\'] header")?.textContent.includes("Unsaved changes")',
            'a saved Timeline edit');
        const saved = await readFile(clipFile, 'utf8');
        assert.notEqual(saved, contents);
        assert.ok(saved.includes(bookmark));
        const updatedClip = (await view.invoke('editor_animation_clips')).find(asset => asset.path === path);
        assert.equal(updatedClip?.asset_id, original.asset_id);
        assert.notEqual(updatedClip.fingerprint, original.fingerprint);

        await view.click('workspace-tabs [role="tab"]', {label: 'Scene'});
        await view.fill('input[aria-label="Search Assets"]', path);
        const asset = '.asset-file[title=' + JSON.stringify(path) + '] button';
        await view.click('editor-assets-panel .more-filters > summary', {label: 'More types'});
        await view.click('editor-assets-panel button[data-asset-kind="animation_clip"]');
        await view.waitFor(textAt('editor-assets-panel .more-filters > summary', 'Animation Clips'), 'the active clip filter');
        await view.click(asset);
        await view.click('.asset-detail-actions button', {label: 'Open Animation Clip'});
        await view.waitFor(textAt(animationHeading, heading), 'existing Clip Timeline opened from Assets');
        await view.waitFor('document.querySelector("[aria-label=\'Timeline bookmarks\'] .keys")?.textContent.includes(' +
            JSON.stringify(bookmark) + ')', 'the saved bookmark restored from file');
        assert.equal(await readFile(clipFile, 'utf8'), saved);
        await view.capture(join(root, 'existing-clip.png'));
    }));


test('Long scene names remain readable and selectable with the keyboard', desktop, () =>
    withEditor('long-scene-picker', async ({view, root}) => {
        await view.resize(640, 500);
        const original = await view.invoke('editor_snapshot');
        await view.click('button[popovertarget="editor-game-menu"]');
        await view.click('#editor-game-menu button', {label: 'New Scene…'});
        await view.fill('#new-scene-name', 'Western Approach Observatory Above the Valley of the Burning Citadel');
        const path = await view.waitFor('document.querySelector("#new-scene-path strong")?.innerText', 'the long scene path');
        assert.ok(path.length > 68);
        await view.click('dialog[open] button', {label: 'Create Scene'});
        const scene = await view.waitFor(predicate('scene', 'scene.active_scene_id !== ' +
            JSON.stringify(original.active_scene_id) + ' && scene'), 'the newly created scene');
        await view.click('workspace-toolbar button', {label: 'Save All'});
        await view.waitFor(predicate('scene', '!scene.dirty && !scene.project_dirty'), 'a saved scene catalog');
        await view.click('button[popovertarget="editor-game-menu"]');
        await view.click('#editor-game-menu button', {label: 'Scene Information…'});
        const picker = 'dialog[open] input[placeholder="Find a scene…"]';
        await view.fill(picker, 'oath.json');
        await view.waitFor(textAt('[aria-label="Scene results"] [aria-selected="true"] > span', 'scenes/oath.json'), 'the startup scene result');
        await view.key('Enter', {code: 'Enter', windowsVirtualKeyCode: 13});
        await view.waitFor(predicate('scene', 'scene.active_scene_id === ' + JSON.stringify(original.active_scene_id)), 'original scene restored');
        if (!await view.evaluate('!!document.querySelector(\'dialog[open] input[placeholder="Find a scene…"]\')')) {
            await view.click('button[popovertarget="editor-game-menu"]');
            await view.click('#editor-game-menu button', {label: 'Scene Information…'});
        }
        await view.fill(picker, path.slice(-24));
        const listed = '[aria-label="Scene results"] [aria-selected="true"] > span';
        await view.waitFor(textAt(listed, path), 'the full long scene result');
        const fits = await view.evaluate('(() => { const list = document.querySelector(\'[aria-label="Scene results"]\');' +
            'const option = list.querySelector(\'[aria-selected="true"]\'); return option.scrollWidth <= option.clientWidth + 1 &&' +
            'list.scrollWidth <= list.clientWidth + 1; })()');
        assert.equal(fits, true);
        await view.capture(join(root, 'long-scene-option.png'));
        await view.key('Enter', {code: 'Enter', windowsVirtualKeyCode: 13});
        await view.waitFor(predicate('state', 'state.active_scene_id === ' + JSON.stringify(scene.active_scene_id)), 'correct long scene opened');
    }));

test('Material Graph edit review stays visible and restores focus in a short window', desktop, () =>
    withEditor('material-review', async ({view, root}) => {
        await view.resize(800, 500);
        const knight = await chooseKnight(view);
        const before = await view.invoke('editor_snapshot');
        const materialButton = 'editor-material-graph > button';
        await view.click(materialButton, {label: 'Create Material Graph'});
        const graph = 'dialog[open] editor-graph';
        await view.click(graph + ' button', {label: 'Add node'});
        await view.waitFor('document.querySelectorAll("dialog[open] editor-graph-node").length > 2', 'an edited graph');
        await view.click('dialog[open] button', {label: 'Close Material Graph'});
        const review = 'dialog[open] .discard-review';
        await view.waitFor(textAt(review + ' [role="status"]', 'Discard the changes to this Material Graph?'), 'visible Material confirmation');
        const onscreen = await view.evaluate('(() => { const review = document.querySelector(".discard-review").getBoundingClientRect(); ' +
            'const body = document.querySelector("dialog[open] .body").getBoundingClientRect();' +
            'return review.top >= body.top - 1 && review.bottom <= body.bottom + 1 && review.bottom <= innerHeight; })()');
        assert.equal(onscreen, true);
        assert.equal(await view.evaluate('document.activeElement?.textContent.trim()'), 'Keep editing');
        await view.capture(join(root, 'discard-review.png'));
        await view.click(review + ' button', {label: 'Keep editing'});
        await view.waitFor('document.activeElement?.classList.contains("graph-workspace")', 'focus back in the edited graph');
        await view.click('dialog[open] button', {label: 'Close Material Graph'});
        await view.click(review + ' button', {label: 'Discard changes'});
        await view.waitFor('!document.querySelector("dialog[open]")', 'Material dialog closed');
        const current = await view.invoke('editor_snapshot');
        assert.equal(current.revision, before.revision);
        assert.deepEqual(current.selection, before.selection);

        await view.click(materialButton, {label: 'Create Material Graph'});
        await view.click('dialog[open] button', {label: 'Apply to Mesh'});
        await view.waitFor('!document.querySelector("dialog[open]")', 'Material Graph applied');
        const edited = await view.invoke('editor_snapshot');
        const material = edited.document.entities.find(entity => entity.key === knight)?.material;
        assert.equal(material?.source_graph?.domain, 'shader');
        assert.deepEqual(edited.document.entities.filter(entity => entity.key !== knight),
            current.document.entities.filter(entity => entity.key !== knight));
    }));

test('Maximize Scene expands the native viewport and restores the ordinary docks', desktop, () =>
    withEditor('maximize-scene', async ({view, root}) => {
        await view.resize(640, 540);
        const selected = await chooseKnight(view);
        const bounds = () => view.evaluate('(() => { const viewport = document.querySelector("scene-viewport").getBoundingClientRect();' +
            'return {width: viewport.width, height: viewport.height}; })()');
        const original = await bounds();
        assert.ok(original.width >= 240 && original.height >= 95, JSON.stringify(original));
        await view.click('editor-viewport-toolbar button', {label: 'Maximize Scene'});
        await view.waitFor('document.querySelector(".scene-maximized")', 'the maximized scene');
        const maximized = await bounds();
        assert.ok(maximized.width > original.width * 1.5);
        assert.ok(maximized.height > original.height + 140);
        assert.equal(await view.evaluate('document.querySelector("editor-assets-panel").getClientRects().length'), 0);
        await view.capture(join(root, 'scene-maximized.png'));
        await view.click('editor-viewport-toolbar button', {label: 'Restore Scene Panels'});
        await view.waitFor('!document.querySelector(".scene-maximized")', 'the restored panels');
        const restored = await bounds();
        assert.ok(Math.abs(restored.width - original.width) < 1 && Math.abs(restored.height - original.height) < 1,
            JSON.stringify({original, restored}));
        assert.deepEqual((await view.invoke('editor_snapshot')).selection, [selected]);
        await view.waitFor(textAt('inspector-panel .object-identity h3', knightName), 'selected Knight restored');
    }));


test('The packaged Editor builds a standalone game with its declared assets', desktop, () =>
    withEditor('standalone-game-export', async ({view, root, project}) => {
        const current = await view.invoke('editor_snapshot');
        const destination = join(root, 'game-exports');
        await mkdir(destination);
        // The paired Angular test covers the workspace menu and native folder picker call.
        // Supply the folder directly here so the host picker cannot touch other desktop windows.
        const build = await view.invoke('editor_prepare_project_build', {
            expectedRevision: current.revision,
            destination
        });
        const finished = await view.invoke('editor_finish_project_build', {buildId: build.build_id});
        const path = join(destination, current.project.name);
        const hash = async file => createHash('sha256').update(await readFile(file)).digest('hex');
        assert.ok(finished.asset_count >= 10);
        assert.equal(await hash(join(path, 'game.exe')), await hash(join(packageDirectory, 'release', 'player', 'game.exe')));
        assert.equal(await hash(join(path, 'assets', 'audio', 'the-vault.wav')),
            await hash(join(project, 'assets', 'audio', 'the-vault.wav')));
        assert.equal((await view.invoke('editor_snapshot')).revision, current.revision);
        await view.waitFor('document.querySelector("editor-project-build")?.textContent.includes("Export complete")',
            'the completed export in the Editor');
    }));


test('Windows Back and Forward traverse Assets and workspace views without leaving the Editor', desktop, () =>
    withEditor('workspace-navigation', async ({view, root}) => {
        await view.resize(760, 540);
        await chooseKnight(view);
        const before = await view.invoke('editor_snapshot');
        const locationIs = label => view.waitFor(textAt('.folder-location [aria-current="location"]', label), 'Assets / ' + label);
        const tabs = 'workspace-tabs [role="tab"]';
        const activeTab = label => view.waitFor(textAt(tabs + '[aria-selected="true"]', label), label + ' workspace');
        const folders = '.asset-grid li button';
        const header = 'editor-header [aria-label="Editor navigation"] button';
        await view.click(folders, {label: 'actors'});
        await locationIs('actors');
        await view.click('.folder-location button', {label: 'Assets'});
        await locationIs('Assets');
        await view.click(folders, {label: 'architecture'});
        await locationIs('architecture');
        await view.click(tabs, {label: 'Animation'});
        await activeTab('Animation');

        await view.key('ArrowLeft', {code: 'ArrowLeft', modifiers: 1, windowsVirtualKeyCode: 37});
        await activeTab('Scene');
        await locationIs('architecture');
        await view.click('.folder-location [aria-current="location"]', {button: 'back'});
        await locationIs('Assets');
        await view.click(header, {label: 'Back'});
        await locationIs('actors');
        await view.click('nav[aria-label="Asset folders"] button', {label: 'Forward'});
        await locationIs('Assets');
        await view.key('ArrowRight', {code: 'ArrowRight', modifiers: 1, windowsVirtualKeyCode: 39});
        await locationIs('architecture');
        await view.click('.folder-location [aria-current="location"]', {button: 'forward'});
        await activeTab('Animation');
        await view.click(header, {label: 'Back'});
        await activeTab('Scene');

        assert.equal(await view.evaluate('location.pathname'), '/editor');
        const after = await view.invoke('editor_snapshot');
        assert.equal(after.revision, before.revision);
        assert.deepEqual(after.selection, before.selection);
        await view.capture(join(root, 'back-in-assets.png'));
    }));

test('Cold startup creates a project without leaving the setup screen in Back history', desktop, () =>
    withEditor('desktop-startup-back', async ({view, root}) => {
        await view.resize(960, 660);
        await view.fill('#project-name', 'Desktop Navigation Test');
        await view.fill('#destination', root);
        await view.click('app-project-setup button[type="submit"]');
        await view.waitFor('location.pathname === "/editor" && !!document.querySelector("editor-workspace")',
            'the newly created project in the Editor');
        const before = await view.invoke('editor_snapshot');
        assert.equal(before.project.name, 'Desktop Navigation Test');
        const editorRoot = 'editor > .editor-main';
        await view.click(editorRoot, {button: 'back'});
        await view.key('BrowserBack', {code: 'BrowserBack', windowsVirtualKeyCode: 166});
        await view.key('ArrowLeft', {code: 'ArrowLeft', modifiers: 1, windowsVirtualKeyCode: 37});
        await view.evaluate('history.back(); true');
        await view.evaluate('new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))');
        assert.equal(await view.evaluate('location.pathname'), '/editor');
        assert.equal(await view.evaluate('!!document.querySelector("app-project-setup")'), false);
        assert.equal((await view.invoke('editor_snapshot')).revision, before.revision);

        await view.click('editor-file-menu button[popovertarget]');
        await view.click('editor-file-menu button', {label: 'New Project…'});
        await view.waitFor('location.pathname === "/setup" && document.querySelector(".back-to-editor")',
            'the explicit New Project screen');
        await view.click('app-project-setup .back-to-editor');
        await view.waitFor('location.pathname === "/editor" && document.querySelector("editor-workspace")',
            'explicit Back to Editor');
        assert.equal((await view.invoke('editor_snapshot')).project.name, 'Desktop Navigation Test');
    }, {openSample: false}));


test('Right-clicking empty workspace chrome offers creation and project actions', desktop, () =>
    withEditor('empty-workspace-create', async ({view, root}) => {
        await view.resize(760, 540);
        const selection = await view.invoke('editor_snapshot');
        const point = await view.waitFor(`(() => {
            const chrome = document.querySelector('.workspace-controls');
            const rect = chrome?.getBoundingClientRect();
            if (!rect) return null;
            for (let x = rect.right - 8; x > rect.left + 7; x -= 8) {
                for (let y = rect.bottom - 6; y > rect.top + 5; y -= 4) {
                    if (document.elementFromPoint(x, y) === chrome) return {x, y};
                }
            }
            return null;
        })()`, 'empty workspace chrome');
        await view.send('Input.dispatchMouseEvent', {type: 'mouseMoved', ...point});
        await view.send('Input.dispatchMouseEvent', {type: 'mousePressed', button: 'right', clickCount: 1, ...point});
        await view.send('Input.dispatchMouseEvent', {type: 'mouseReleased', button: 'right', clickCount: 1, ...point});
        const menu = '[role="menu"][aria-label="Workspace actions"]';
        await view.waitFor('!!document.querySelector(' + JSON.stringify(menu) + ')', 'workspace context menu');
        const list = await view.evaluate('[...document.querySelectorAll(' + JSON.stringify(menu + ' [role="menuitem"]') +
            ')].map(button => button.textContent.trim())');
        for (const action of ['Create Object', 'Build Game…', 'New Scene…', 'Play']) {
            assert.ok(list.includes(action), 'Missing workspace action ' + action + ' in ' + JSON.stringify(list));
        }
        await view.click(menu + ' [role="menuitem"]', {label: 'Create Object'});
        const create = '[role="menu"][aria-label="Create Object"]';
        await view.click(create + ' [role="menuitem"]', {label: 'Empty Object'});
        const created = await view.waitFor(predicate('scene', 'scene.document.entities.length > ' +
            selection.document.entities.length + ' && scene'), 'object created from the workspace menu');
        assert.equal(created.selection.length, 1);
        await view.waitFor('!!document.querySelector("inspector-panel .object-identity h3")', 'created object in Inspector');
        await view.capture(join(root, 'workspace-context-object.png'));
    }));


test('Editor Back returns to the previous scene and reviews unsaved edits', desktop, () =>
    withEditor('scene-navigation-history', async ({view}) => {
        await view.resize(900, 640);
        const before = await view.invoke('editor_snapshot');
        const other = before.scenes.find(scene => scene.id !== before.active_scene_id);
        assert.ok(other);
        await view.fill('input[aria-label="Search Assets"]', other.asset_path);
        await view.click('.asset-file[title=' + JSON.stringify(other.asset_path) + '] button');
        await view.click('.asset-detail-actions button', {label: 'Open Scene'});
        const isScene = id => predicate('scene', 'scene.active_scene_id === ' + JSON.stringify(id));
        await view.waitFor(isScene(other.id), 'the second scene');
        const cube = await createCube(view);
        const header = 'editor-header [aria-label="Editor navigation"] button';
        await view.click(header, {label: 'Back'});
        const prompt = 'dialog[open]:has(.switch-prompt)';
        await view.waitFor('!!document.querySelector(' + JSON.stringify(prompt) + ')', 'the scene change review');
        await view.click(prompt + ' button', {label: 'Keep Edits and Open'});
        await view.waitFor(isScene(before.active_scene_id), 'the preceding scene');
        await view.click(header, {label: 'Forward'});
        await view.waitFor(isScene(other.id), 'the second scene restored');
        const after = await view.invoke('editor_snapshot');
        assert.ok(after.document.entities.some(entity => entity.key === cube));
        assert.equal(after.dirty, true);
        assert.equal(await view.evaluate('location.pathname'), '/editor');
    }));

