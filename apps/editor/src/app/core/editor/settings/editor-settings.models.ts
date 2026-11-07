import {editorShortcutHelp, type EditorShortcutHelp, NATIVE_SCENE_NAVIGATION} from '../editor-keymap';

export type EditorSettingsPage = 'editor' | 'editor.keymap';

export interface EditorSettingsState {
    readonly open: boolean;
    readonly page: EditorSettingsPage;
}

interface EditorSettingsNode {
    readonly id: EditorSettingsPage;
    readonly label: string;
    readonly children?: readonly EditorSettingsNode[];
}

export const EDITOR_SETTINGS_TREE: readonly EditorSettingsNode[] = [
    {id: 'editor', label: 'Editor', children: [{id: 'editor.keymap', label: 'Keymap'}]},
];

interface EditorKeymapSection {
    readonly title: string;
    readonly context: string;
    readonly shortcuts: readonly EditorShortcutHelp[];
}

export const EDITOR_KEYMAP_SECTIONS: readonly EditorKeymapSection[] = [
    {
        title: 'Workspace',
        context: 'Available across the Scene, Hierarchy, Inspector, and Assets panes.',
        shortcuts: editorShortcutHelp('workspace'),
    },
    {
        title: 'Scene tools and views',
        context: 'Click the Scene viewport to focus it.',
        shortcuts: editorShortcutHelp('scene').filter(shortcut => shortcut.id.startsWith('scene.')),
    },
    {
        title: 'Scene navigation',
        context: 'In Walk or Fly mode, movement keys control the camera.',
        shortcuts: NATIVE_SCENE_NAVIGATION,
    },
    {
        title: 'Entity selection',
        context: 'Available in the Scene and Hierarchy.',
        shortcuts: editorShortcutHelp('hierarchy').filter(shortcut => shortcut.id.startsWith('selection.')),
    },
    {
        title: 'Hierarchy tree',
        context: 'Use these keys with a Hierarchy row focused.',
        shortcuts: [
            {id: 'row-step', label: 'Focus the previous or next row', keys: 'Up / Down'},
            {id: 'row-edge', label: 'Focus the first or last row', keys: 'Home / End'},
            {id: 'row-collapse', label: 'Collapse a branch or focus its parent', keys: 'Left'},
            {id: 'row-expand', label: 'Expand a branch or focus its first child', keys: 'Right'},
            {id: 'row-select', label: 'Select the focused row', keys: 'Enter / Space'},
        ],
    },
];
