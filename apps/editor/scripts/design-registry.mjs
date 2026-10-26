import {watch} from 'node:fs';
import {mkdir, readdir, readFile, writeFile} from 'node:fs/promises';
import {dirname, join, relative} from 'node:path';
import ts from 'typescript';

function angularComponents(source, file) {
    const ast = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true);
    const decorators = new Set();
    for (const statement of ast.statements) {
        if (!ts.isImportDeclaration(statement) || statement.moduleSpecifier.text !== '@angular/core')
            continue;
        const bindings = statement.importClause?.namedBindings;
        if (!bindings || !ts.isNamedImports(bindings)) continue;
        for (const binding of bindings.elements) {
            if ((binding.propertyName ?? binding.name).text === 'Component')
                decorators.add(binding.name.text);
        }
    }
    return ast.statements
        .filter(
            (node) =>
                ts.isClassDeclaration(node) &&
                node.modifiers?.some((modifier) => modifier.kind === ts.SyntaxKind.ExportKeyword) &&
                ts
                    .getDecorators(node)
                    ?.some(
                        (decorator) =>
                            ts.isCallExpression(decorator.expression) &&
                            ts.isIdentifier(decorator.expression.expression) &&
                            decorators.has(decorator.expression.expression.text),
                    ),
        )
        .map((node) => ({
            file,
            exportName: node.modifiers.some((modifier) => modifier.kind === ts.SyntaxKind.DefaultKeyword)
                ? 'default'
                : node.name?.text,
            requiredInputs: node.members.flatMap((member) => {
                const call = member.initializer;
                if (
                    !call ||
                    !ts.isCallExpression(call) ||
                    !ts.isPropertyAccessExpression(call.expression) ||
                    call.expression.name.text !== 'required' ||
                    !['input', 'model'].includes(call.expression.expression.getText(ast))
                )
                    return [];
                const options = call.arguments[0];
                const alias =
                    options && ts.isObjectLiteralExpression(options)
                        ? options.properties.find(
                            (property) =>
                                ts.isPropertyAssignment(property) && property.name.getText(ast) === 'alias',
                        )
                        : undefined;
                return [
                    alias && ts.isStringLiteral(alias.initializer)
                        ? alias.initializer.text
                        : member.name.getText(ast),
                ];
            }),
        }))
        .filter((component) => component.exportName);
}

const label = (id) =>
    id.replace(/[-_]+/g, ' ').replace(/\b\w/g, (character) => character.toUpperCase());

export async function generateRegistry(componentRoot, outputFile) {
    const folders = (await readdir(componentRoot, {withFileTypes: true}))
        .filter((entry) => entry.isDirectory() && !entry.name.startsWith('.'))
        .sort((a, b) => a.name.localeCompare(b.name));
    const entries = [];
    const importPath = (file) => {
        const path = relative(dirname(outputFile), file).replaceAll('\\', '/').replace(/\.ts$/, '');
        return JSON.stringify(path.startsWith('.') ? path : `./${path}`);
    };
    for (const folder of folders) {
        const directory = join(componentRoot, folder.name);
        const files = await readdir(directory);
        const preview = files.includes(`${folder.name}.design.ts`)
            ? join(directory, `${folder.name}.design.ts`)
            : undefined;
        const components = [];
        for (const file of files.filter(
            (name) => name.endsWith('.ts') && !/\.(?:spec|test|design|d)\.ts$/.test(name),
        )) {
            const path = join(directory, file);
            components.push(...angularComponents(await readFile(path, 'utf8'), path));
        }
        // A custom preview can select a component when the folder contains several.
        const component = components.length === 1 ? components[0] : undefined;
        let load;
        if (component && preview) {
            load = `async () => { const [module, preview] = await Promise.all([import(${importPath(component.file)}), import(${importPath(preview)})]); const options: ComponentPreview = preview.default; return {requiredInputs: options.component ? [] : ${JSON.stringify(component.requiredInputs)}, ...options, component: options.component ?? module[${JSON.stringify(component.exportName)}]}; }`;
        } else if (component) {
            load = `() => import(${importPath(component.file)}).then(module => ({component: module[${JSON.stringify(component.exportName)}]}))`;
        } else if (preview) {
            load = `() => import(${importPath(preview)}).then(preview => preview.default)`;
        }
        entries.push(
            `    {id: ${JSON.stringify(folder.name)}, title: ${JSON.stringify(label(folder.name))}, requiredInputs: ${JSON.stringify(component?.requiredInputs ?? [])}${load ? `, load: ${load}` : ''}},`,
        );
    }
    const source =
        '// Generated by scripts/design-registry.mjs. Do not edit by hand.\n' +
        "import type {ComponentPreview, ComponentSection} from './component-registry';\n\n" +
        'export const componentRegistry: readonly ComponentSection[] = [\n' +
        entries.join('\n') +
        '\n];\n';
    await mkdir(dirname(outputFile), {recursive: true});
    const previous = await readFile(outputFile, 'utf8').catch((error) => {
        if (error.code === 'ENOENT') return '';
        throw error;
    });
    if (previous !== source) await writeFile(outputFile, source);
    return previous !== source;
}

export function watchRegistry(
    componentRoot,
    outputFile,
    onError = (error) => console.error(error),
) {
    let timer;
    let pending = Promise.resolve();
    const watcher = watch(componentRoot, {recursive: true}, () => {
        clearTimeout(timer);
        timer = setTimeout(() => {
            pending = pending.then(() => generateRegistry(componentRoot, outputFile)).catch(onError);
        }, 100);
    });
    watcher.on('error', onError);
    return () => {
        clearTimeout(timer);
        watcher.close();
    };
}
