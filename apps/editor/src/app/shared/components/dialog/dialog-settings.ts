import {Config, isConfigObject} from '../../models/config.model';
import type {FieldTree} from '@angular/forms/signals';
import type {EditorIconName} from '../../icons/editor-icon';
import type {SubjectInput} from '../../../core/subjects/subject.models';

export interface DialogSettings {
    width: string;
    height: string;
    minWidth: string;
    minHeight: string;
    maxWidth: string;
    maxHeight: string;
    draggable: boolean;
    resizable: boolean;
    expandable: boolean;
    expanded: boolean;
    closeOnEscape: boolean;
    closeOnBackdrop: boolean;
    x: number;
    y: number;
}

export interface DialogSubject {
    readonly id: string;
    readonly title: string;
    readonly icon?: EditorIconName;
    readonly form: FieldTree<unknown>;
    readonly inputs?: readonly SubjectInput[];
}

export const DEFAULT_DIALOG_SETTINGS: Readonly<DialogSettings> = {
    width: 'var(--layout-dialog-width)',
    height: 'auto',
    minWidth: 'var(--layout-dialog-min-width)',
    minHeight: 'var(--layout-dialog-min-height)',
    maxWidth: 'var(--layout-dialog-max-width)',
    maxHeight: 'var(--layout-dialog-max-height)',
    draggable: true,
    resizable: true,
    expandable: true,
    expanded: false,
    closeOnEscape: true,
    closeOnBackdrop: false,
    x: 0,
    y: 0,
};

export function dialogSettings(value: unknown): Config<DialogSettings> {
    if (!isConfigObject(value)) return {};
    const config: Config<DialogSettings> = {};
    for (const name of [
        'width',
        'height',
        'minWidth',
        'minHeight',
        'maxWidth',
        'maxHeight',
    ] as const) {
        const size = value[name];
        if (
            typeof size === 'string' &&
            /^(?:auto|0|\d+(?:\.\d+)?(?:px|rem|em|%|vw|vh|dvw|dvh)|var\(--[a-zA-Z0-9-]+\))$/.test(size)
        )
            config[name] = size;
    }
    for (const name of [
        'draggable',
        'resizable',
        'expandable',
        'expanded',
        'closeOnEscape',
        'closeOnBackdrop',
    ] as const) {
        if (typeof value[name] === 'boolean') config[name] = value[name];
    }
    for (const name of ['x', 'y'] as const) {
        if (typeof value[name] === 'number' && Number.isFinite(value[name])) config[name] = value[name];
    }
    return config;
}
