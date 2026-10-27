import assert from 'node:assert/strict';
import {mkdir, mkdtemp, readFile, rm, writeFile} from 'node:fs/promises';
import {join, resolve, sep} from 'node:path';
import {test} from 'node:test';
import {generateRegistry, watchRegistry} from './design-registry.mjs';

async function removeFixture(path) {
    assert.ok(resolve(path).startsWith(resolve('tmp') + sep));
    await rm(path, {recursive: true, force: true});
}

async function fixture(t) {
    await mkdir('tmp', {recursive: true});
    const directory = await mkdtemp(resolve('tmp/design-registry-'));
    t.after(() => removeFixture(directory));
    const root = join(directory, 'components');
    const output = join(directory, 'design/registry.ts');
    await mkdir(root);
    const add = async (path, source = '') => {
        const file = join(root, path);
        await mkdir(resolve(file, '..'), {recursive: true});
        await writeFile(file, source);
    };
    const preview = async (name, source) => {
        const file = join(directory, 'design/previews', `${name}.design.ts`);
        await mkdir(resolve(file, '..'), {recursive: true});
        await writeFile(file, source);
        return file;
    };
    return {root, output, add, preview, source: () => readFile(output, 'utf8')};
}

test('each direct folder becomes a sorted section, including folders without a component', async (t) => {
    const f = await fixture(t);
    await f.add(
        'splitter/splitter.ts',
        "import {Component} from '@angular/core'; @Component({}) export class Splitter {}",
    );
    await f.add(
        'drawer/drawer.ts',
        "import {Component} from '@angular/core'; @Component({}) export class Drawer {}",
    );
    await f.add('drawer/model/options.ts', 'export interface Options {}');
    await f.add(
        'drawer/drawer.spec.ts',
        "import {Component} from '@angular/core'; @Component({}) export class TestOnly {}",
    );
    await mkdir(join(f.root, 'empty'));
    await f.add('helper.ts', 'export const value = 1;');
    await generateRegistry(f.root, f.output);
    const source = await f.source();
    assert.ok(source.indexOf('id: "drawer"') < source.indexOf('id: "empty"'));
    assert.ok(source.indexOf('id: "empty"') < source.indexOf('id: "splitter"'));
    assert.match(source, /module\["Drawer"\]/);
    assert.doesNotMatch(source, /TestOnly|options|id: "helper"/);
    assert.equal(
        await generateRegistry(f.root, f.output),
        false,
        'Unchanged registries must not trigger rebuilds.',
    );
});

test('detects aliased component decorators, required input aliases, and preview metadata', async (t) => {
    const f = await fixture(t);
    await f.add(
        'pane.scss/pane.scss.ts',
        `import {Component as View, input, model} from '@angular/core';
        @View({}) export class Pane {
            readonly label = input.required<string>({alias: 'title'});
            readonly value = model.required<number>();
        }`,
    );
    await f.preview('pane.scss', 'export default {inputs: {title: "Pane", value: 0}};');
    await generateRegistry(f.root, f.output);
    const source = await f.source();
    assert.match(source, /requiredInputs: \["title","value"\]/);
    assert.match(source, /pane\.scss\.design/);
    assert.match(source, /ComponentPreview = preview.default/);
});

test('supports default-exported components and explicit previews for multi-component folders', async (t) => {
    const f = await fixture(t);
    await f.add(
        'button/button.ts',
        "import {Component} from '@angular/core'; @Component({}) export default class Button {}",
    );
    await f.add(
        'custom/parts.ts',
        "import {Component} from '@angular/core'; @Component({}) export class A {} @Component({}) export class B {}",
    );
    await f.preview(
        'custom',
        "import {B} from '../../components/custom/parts'; export default {component: B};",
    );
    await generateRegistry(f.root, f.output);
    const source = await f.source();
    assert.match(source, /module\["default"\]/);
    assert.match(source, /custom\.design/);
    assert.doesNotMatch(source, /module\["A"\]/);
});

test('watch mode adds and removes sections while serving', async (t) => {
    const f = await fixture(t);
    await generateRegistry(f.root, f.output);
    const errors = [];
    const stop = watchRegistry(f.root, f.output, (error) => errors.push(error));
    const until = async (predicate) => {
        const deadline = Date.now() + 5000;
        while (!predicate(await f.source())) {
            assert.ok(Date.now() < deadline, 'Registry watcher did not update.');
            await new Promise((resolve) => setTimeout(resolve, 25));
        }
    };
    try {
        await f.add(
            'new-pane.scss/new-pane.scss.ts',
            "import {Component} from '@angular/core'; @Component({}) export class NewPane {}",
        );
        await until((source) => source.includes('module["NewPane"]'));
        const preview = await f.preview(
            'new-pane.scss',
            'export default {inputs: {title: "Preview"}};',
        );
        await until((source) => source.includes('previews/new-pane.scss.design'));
        await removeFixture(preview);
        await until((source) => !source.includes('previews/new-pane.scss.design'));
        await removeFixture(join(f.root, 'new-pane.scss'));
        await until((source) => !source.includes('id: "new-pane.scss"'));
        assert.deepEqual(errors, []);
    } finally {
        stop();
    }
});
