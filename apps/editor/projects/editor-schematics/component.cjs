const {
    apply,
    applyTemplates,
    mergeWith,
    move,
    SchematicsException,
    strings,
    url
} = require('@angular-devkit/schematics');

function generateComponent({name}, {root, prefix, panel}) {
    return (tree) => {
        const {subdirs, subfiles} = tree.getDir(root);
        const destination = `${root}/${name}`;
        if ([...subdirs, ...subfiles].some((entry) => entry.toLowerCase() === name)) {
            throw new SchematicsException(`Component destination already exists: ${destination}`);
        }

        const title = name.split('-').map((part) => part[0].toUpperCase() + part.slice(1)).join(' ');
        return mergeWith(apply(url('./files'), [
            applyTemplates({name, panel, title, selector: `${prefix}-${name}`, className: strings.classify(name)}),
            move(destination),
        ]));
    };
}

exports.uiComponent = (options) => generateComponent(options, {
    root: 'src/app/shared/components',
    prefix: 'app',
    panel: false,
});

exports.editorPanel = (options) => generateComponent(options, {
    root: 'src/app/modules/editor/workspace',
    prefix: 'editor',
    panel: true,
});
