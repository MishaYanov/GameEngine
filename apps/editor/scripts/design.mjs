import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {join} from 'node:path';
import {generateRegistry, watchRegistry} from './design-registry.mjs';

const workspace = fileURLToPath(new URL('..', import.meta.url));
const components = join(workspace, 'src/app/shared/components');
const registry = join(workspace, 'projects/design/src/component-registry.generated.ts');
const args = process.argv.slice(2);

await generateRegistry(components, registry);
if (args.length) {
    const stop =
        args[0] === 'serve' || args.includes('--watch')
            ? watchRegistry(components, registry)
            : () => undefined;
    const child = spawn(
        process.execPath,
        [join(workspace, 'node_modules/@angular/cli/bin/ng.js'), ...args],
        {
            cwd: workspace,
            stdio: 'inherit',
            windowsHide: true,
        },
    );
    child.on('error', (error) => {
        stop();
        console.error(error);
        process.exitCode = 1;
    });
    child.on('exit', (code, signal) => {
        stop();
        process.exitCode = code ?? (signal ? 1 : 0);
    });
    for (const signal of ['SIGINT', 'SIGTERM'])
        process.on(signal, () => {
            stop();
            child.kill(signal);
        });
}
