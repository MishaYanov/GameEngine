import {EDITOR_ACTIONS, type EditorActionId} from './editor-actions';
import type {ViewportInput} from './editor.models';

export type EditorShortcutScope = 'workspace' | 'scene' | 'hierarchy' | 'inspector' | 'assets';
type Modifiers = 'plain' | 'control' | 'primary' | 'primary-shift';

export interface EditorShortcutHelp {
    readonly id: string;
    readonly label: string;
    readonly keys: string;
}

interface EditorShortcut {
    readonly action: EditorActionId;
    readonly scopes: readonly EditorShortcutScope[];
    readonly code: string;
    readonly key?: string;
    readonly modifiers?: Modifiers;
    readonly label: string;
}

const scene = ['scene'] as const;
const selection = ['scene', 'hierarchy'] as const;
const documentScopes = ['workspace', 'scene', 'hierarchy', 'inspector', 'assets'] as const;

/** Physical key codes match the native Scene surface. A key fallback is for DOM input without a code. */
export const EDITOR_KEYMAP: readonly EditorShortcut[] = [
    {action: 'scene.tool.hand', scopes: scene, code: 'KeyQ', key: 'q', label: 'Q'},
    {action: 'scene.tool.move', scopes: scene, code: 'KeyW', key: 'w', label: 'W'},
    {action: 'scene.tool.rotate', scopes: scene, code: 'KeyE', key: 'e', label: 'E'},
    {action: 'scene.tool.scale', scopes: scene, code: 'KeyR', key: 'r', label: 'R'},
    {action: 'scene.frame', scopes: scene, code: 'KeyF', key: 'f', label: 'F'},
    {action: 'scene.frame', scopes: scene, code: 'NumpadDecimal', label: 'Numpad .'},
    {action: 'scene.zoom-in', scopes: scene, code: 'NumpadAdd', label: 'Numpad +'},
    {action: 'scene.zoom-out', scopes: scene, code: 'NumpadSubtract', label: 'Numpad −'},
    {action: 'scene.front', scopes: scene, code: 'Numpad1', label: 'Numpad 1'},
    {action: 'scene.back', scopes: scene, code: 'Numpad1', modifiers: 'control', label: 'Ctrl + Numpad 1'},
    {action: 'scene.right', scopes: scene, code: 'Numpad3', label: 'Numpad 3'},
    {action: 'scene.left', scopes: scene, code: 'Numpad3', modifiers: 'control', label: 'Ctrl + Numpad 3'},
    {action: 'scene.top', scopes: scene, code: 'Numpad7', label: 'Numpad 7'},
    {action: 'scene.bottom', scopes: scene, code: 'Numpad7', modifiers: 'control', label: 'Ctrl + Numpad 7'},
    {action: 'scene.projection', scopes: scene, code: 'Numpad5', label: 'Numpad 5'},
    {action: 'scene.cancel', scopes: scene, code: 'Escape', key: 'escape', label: 'Esc'},
    {
        action: 'document.save',
        scopes: documentScopes,
        code: 'KeyS',
        key: 's',
        modifiers: 'primary',
        label: 'Ctrl/Cmd + S'
    },
    {
        action: 'document.undo',
        scopes: documentScopes,
        code: 'KeyZ',
        key: 'z',
        modifiers: 'primary',
        label: 'Ctrl/Cmd + Z'
    },
    {
        action: 'document.redo',
        scopes: documentScopes,
        code: 'KeyY',
        key: 'y',
        modifiers: 'primary',
        label: 'Ctrl/Cmd + Y'
    },
    {
        action: 'document.redo',
        scopes: documentScopes,
        code: 'KeyZ',
        key: 'z',
        modifiers: 'primary-shift',
        label: 'Ctrl/Cmd + Shift + Z'
    },
    {action: 'selection.all', scopes: selection, code: 'KeyA', key: 'a', modifiers: 'primary', label: 'Ctrl/Cmd + A'},
    {
        action: 'selection.duplicate',
        scopes: selection,
        code: 'KeyD',
        key: 'd',
        modifiers: 'primary',
        label: 'Ctrl/Cmd + D'
    },
    {action: 'selection.delete', scopes: selection, code: 'Delete', key: 'delete', label: 'Delete'},
];

/** Display bindings handled directly by the native viewport. */
export const NATIVE_SCENE_NAVIGATION: readonly EditorShortcutHelp[] = [
    {id: 'orbit-drag', label: 'Orbit', keys: 'Middle drag / Alt + left drag'},
    {id: 'pan-drag', label: 'Pan', keys: 'Shift + middle drag / Hand tool + left drag'},
    {id: 'dolly-drag', label: 'Dolly / zoom', keys: 'Ctrl + middle drag / Scroll'},
    {id: 'orbit-step', label: 'Orbit in steps', keys: 'Numpad 2 / 4 / 6 / 8'},
    {id: 'orbit-flip', label: 'Flip the view', keys: 'Numpad 9'},
    {id: 'walk', label: 'Enter Walk mode', keys: 'Shift + backtick'},
    {id: 'walk-look', label: 'Look around in Walk or Fly', keys: 'Mouse movement'},
    {id: 'walk-move', label: 'Move in Walk or Fly', keys: 'W / A / S / D'},
    {id: 'walk-height', label: 'Move down / up in Walk or Fly', keys: 'Q / E'},
    {id: 'walk-confirm', label: 'Keep the Walk view', keys: 'Enter / Left click'},
    {id: 'walk-cancel', label: 'Restore the starting Walk view', keys: 'Esc / Right click'},
    {id: 'fly', label: 'Fly while held', keys: 'Right mouse button'},
];

const WIDGET_OWNED_INPUT = [
    'input', 'textarea', 'select', '[contenteditable]', '[role="textbox"]', '[role="combobox"]',
    'dialog', '[role="dialog"]', '[role="menu"]', '[role="listbox"]', '[popover]',
].join(', ');

type KeyStroke = Pick<KeyboardEvent, 'code' | 'key' | 'ctrlKey' | 'metaKey' | 'shiftKey'>;

function matchingModifiers(event: KeyStroke, modifiers: Modifiers = 'plain'): boolean {
    const primary = event.ctrlKey !== event.metaKey;
    switch (modifiers) {
        case 'plain':
            return !event.ctrlKey && !event.metaKey && !event.shiftKey;
        case 'control':
            return event.ctrlKey && !event.metaKey && !event.shiftKey;
        case 'primary':
            return primary && !event.shiftKey;
        case 'primary-shift':
            return primary && event.shiftKey;
    }
}

function matchKeymap(event: KeyStroke, scope: string): EditorActionId | undefined {
    return EDITOR_KEYMAP.find(binding =>
        binding.scopes.some(allowed => allowed === scope) && matchingModifiers(event, binding.modifiers) &&
        (event.code && event.code !== 'Unidentified' ? event.code === binding.code : event.key.toLowerCase() === binding.key),
    )?.action;
}

/** Called from a workspace's bubbling listener so the focused widget has first refusal. */
export function resolveEditorShortcut(event: KeyboardEvent): EditorActionId | undefined {
    if (event.defaultPrevented || event.isComposing || event.repeat || event.altKey || event.getModifierState('AltGraph')) return;
    const target = event.composedPath().find((entry): entry is Element => entry instanceof Element);
    if (!target || target.closest(WIDGET_OWNED_INPUT)) return;
    const owner = target.ownerDocument;
    if (owner.querySelector('dialog[open], [role="dialog"][aria-modal="true"]:not([hidden])')) return;
    if (owner.defaultView?.CSS?.supports?.('selector(:popover-open)') && owner.querySelector(':popover-open')) return;
    const scope = target.closest('[data-editor-shortcut-scope]')?.getAttribute('data-editor-shortcut-scope');
    return scope ? matchKeymap(event, scope) : undefined;
}

/** The native worker handles local navigation, tools, history, and selection itself. */
export function resolveNativeEditorShortcut(input: ViewportInput): 'document.save' | undefined {
    if (input.kind !== 'key_down' || input.repeat || input.modifiers.alt) return;
    const action = matchKeymap({
        code: input.code ?? '', key: '', ctrlKey: input.modifiers.control,
        metaKey: input.modifiers.super_key, shiftKey: input.modifiers.shift,
    }, 'scene');
    return action === 'document.save' ? action : undefined;
}

export function editorShortcutLabel(action: EditorActionId): string {
    return EDITOR_KEYMAP.find(binding => binding.action === action)?.label ?? '';
}

export function editorShortcutHelp(scope: EditorShortcutScope): readonly (EditorShortcutHelp & {
    id: EditorActionId
})[] {
    const byAction = new Map<EditorActionId, string[]>();
    for (const binding of EDITOR_KEYMAP) {
        if (!binding.scopes.includes(scope)) continue;
        const keys = byAction.get(binding.action);
        if (keys) keys.push(binding.label);
        else byAction.set(binding.action, [binding.label]);
    }
    return Array.from(byAction, ([id, keys]) => ({id, label: EDITOR_ACTIONS[id].label, keys: keys.join(' / ')}));
}
