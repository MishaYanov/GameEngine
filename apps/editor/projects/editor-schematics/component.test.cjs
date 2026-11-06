const assert = require('node:assert/strict');
const path = require('node:path');
const test = require('node:test');
const {HostTree} = require('@angular-devkit/schematics');
const {SchematicTestRunner} = require('@angular-devkit/schematics/testing');

function runner() {
    return new SchematicTestRunner('editor', path.join(__dirname, 'collection.json'));
}

test('generates a standalone component in a design guide discovery folder', async () => {
    const root = '/src/app/shared/components/asset-badge/asset-badge';
    const tree = await runner().runSchematic('ui-component', {name: 'asset-badge'});

    assert.deepEqual(tree.files.sort(), ['.html', '.scss', '.ts'].map((ext) => root + ext));
    assert.match(tree.readText(`${root}.ts`), /selector: 'app-asset-badge'/);
    assert.match(tree.readText(`${root}.ts`), /export class AssetBadge/);
});

test('generates an editor panel with the shared pane', async () => {
    const root = '/src/app/modules/editor/workspace/scene-tools-panel/scene-tools-panel';
    const tree = await runner().runSchematic('editor-panel', {name: 'scene-tools-panel'});

    assert.deepEqual(tree.files.sort(), ['.html', '.scss', '.ts'].map((ext) => root + ext));
    assert.match(tree.readText(`${root}.ts`), /selector: 'editor-scene-tools-panel'/);
    assert.match(tree.readText(`${root}.html`), /<editor-pane \[title\]="title\(\)">/);
});

test('rejects unsafe names and protects existing components', async () => {
    const schematics = runner();
    for (const name of ['../asset-badge', 'AssetBadge', 'asset--badge', 'con']) {
        await assert.rejects(schematics.runSchematic('ui-component', {name}), /pattern/);
    }

    const tree = new HostTree();
    tree.create('/src/app/shared/components/Asset-Badge/custom.ts', 'keep');
    await assert.rejects(schematics.runSchematic('ui-component', {name: 'asset-badge'}, tree), /already exists/);
    assert.equal(tree.readText('/src/app/shared/components/Asset-Badge/custom.ts'), 'keep');
});
