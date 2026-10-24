import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';

const stats = JSON.parse(
    await readFile(new URL('../dist/editor/stats.json', import.meta.url), 'utf8'),
);
const inputs = Object.keys(stats.inputs).map((path) => path.replaceAll('\\', '/'));
const forbidden = inputs.filter((path) =>
    /(?:projects\/design\/|\.design\.ts$|app\.routes\.development|environment\.(?:development|staging))/.test(
        path,
    ),
);
assert.deepEqual(
    forbidden,
    [],
    `Development tools leaked into production: ${forbidden.join(', ')}`,
);
assert.ok(
    inputs.some((path) => path.endsWith('src/app/modules/project-setup/project-setup.ts')),
    'The production project setup must remain bundled.',
);
assert.ok(
    inputs.some((path) => path.endsWith('src/app/modules/editor/editor.ts')),
    'The editor route must remain bundled.',
);
console.log(
    'Production bundle verified: project setup included; design and non-production entry points excluded.',
);
