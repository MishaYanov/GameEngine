import {resolveEditorShortcut, resolveNativeEditorShortcut} from './editor-keymap';
import type {EditorActionId} from './editor-actions';
import type {ViewportInput} from './editor.models';

function resolve(scope: string, init: KeyboardEventInit, tag = 'button', claimed = false): EditorActionId | undefined {
    const pane = document.createElement('section');
    pane.setAttribute('data-editor-shortcut-scope', scope);
    const target = document.createElement(tag);
    pane.append(target);
    let action: EditorActionId | undefined;
    pane.addEventListener('keydown', event => action = resolveEditorShortcut(event));
    if (claimed) target.addEventListener('keydown', event => event.preventDefault());
    target.dispatchEvent(new KeyboardEvent('keydown', {bubbles: true, cancelable: true, ...init}));
    return action;
}

describe('Editor keymap ownership', () => {
    it('resolves only the actions allowed by the focused pane and exact modifier chord', () => {
        expect(resolve('scene', {code: 'KeyW', key: 'w'})).toBe('scene.tool.move');
        expect(resolve('hierarchy', {code: 'KeyW', key: 'w'})).toBeUndefined();
        expect(resolve('scene', {code: 'KeyW', key: 'W', shiftKey: true})).toBeUndefined();
        expect(resolve('scene', {code: 'Numpad1', key: 'End', ctrlKey: true})).toBe('scene.back');
        expect(resolve('scene', {code: 'Digit1', key: '1'})).toBeUndefined();
        expect(resolve('hierarchy', {code: 'KeyD', key: 'd', ctrlKey: true})).toBe('selection.duplicate');
        expect(resolve('assets', {code: 'Delete', key: 'Delete'})).toBeUndefined();
        expect(resolve('inspector', {code: 'KeyZ', key: 'Z', metaKey: true, shiftKey: true})).toBe('document.redo');
        expect(resolve('game', {code: 'KeyZ', key: 'z', ctrlKey: true})).toBeUndefined();
        expect(resolve('assets', {code: 'KeyP', key: 'P', ctrlKey: true, shiftKey: true})).toBe('palette.open');
        expect(resolve('workspace', {key: 'P', metaKey: true, shiftKey: true})).toBe('palette.open');
        expect(resolve('workspace', {code: 'KeyP', key: 'p', ctrlKey: true})).toBeUndefined();
    });

    it('preserves text entry, composition, focused widget handlers, and modal input', () => {
        const undo = {code: 'KeyZ', key: 'z', ctrlKey: true};
        expect(resolve('scene', undo, 'input')).toBeUndefined();
        expect(resolve('scene', {...undo, isComposing: true})).toBeUndefined();
        expect(resolve('scene', {...undo, repeat: true})).toBeUndefined();
        expect(resolve('scene', undo, 'button', true)).toBeUndefined();
        const modal = document.createElement('dialog');
        modal.open = true;
        document.body.append(modal);
        try {
            expect(resolve('scene', undo)).toBeUndefined();
        } finally {
            modal.remove();
        }
    });

    it('forwards commands the native surface delegates to the workspace', () => {
        const input: ViewportInput = {
            kind: 'key_down', code: 'KeyS', position: null, extent: [640, 480], button: null, repeat: false,
            modifiers: {control: true, super_key: false, alt: false, shift: false}, delta: null,
        };
        expect(resolveNativeEditorShortcut(input)).toBe('document.save');
        expect(resolveNativeEditorShortcut({...input, code: 'KeyP'})).toBeUndefined();
        expect(resolveNativeEditorShortcut({...input, code: 'KeyP', modifiers: {...input.modifiers, shift: true}}))
            .toBe('palette.open');
        expect(resolveNativeEditorShortcut({...input, code: 'KeyZ'})).toBeUndefined();
        expect(resolveNativeEditorShortcut({...input, code: 'KeyD'})).toBeUndefined();
        expect(resolveNativeEditorShortcut({...input, kind: 'key_up'})).toBeUndefined();
        expect(resolveNativeEditorShortcut({...input, repeat: true})).toBeUndefined();
    });
});
