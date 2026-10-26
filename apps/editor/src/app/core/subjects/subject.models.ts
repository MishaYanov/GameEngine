import type {EditorIconName} from '../../shared/icons/editor-icon';

export interface Vector3Value {
    x: number;
    y: number;
    z: number;
}

export interface ObjectSubjectData {
    name: string;
    visible: boolean;
}

export interface TransformSubjectData {
    position: Vector3Value;
    rotation: Vector3Value;
    scale: Vector3Value;
}

export type ScriptPropertyValue = string | number | boolean | Vector3Value;
export type ScriptPropertyDefinition = (
    | { readonly name: string; readonly type: 'number'; readonly defaultValue: number }
    | { readonly name: string; readonly type: 'string'; readonly defaultValue: string }
    | { readonly name: string; readonly type: 'boolean'; readonly defaultValue: boolean }
    | { readonly name: string; readonly type: 'vector3'; readonly defaultValue: Vector3Value }
    ) & { readonly underExpanded?: boolean };

export interface ScriptReference {
    readonly assetId: string;
    readonly path: string;
    readonly className: string;
}

export interface ScriptAsset extends ScriptReference {
    readonly properties: readonly ScriptPropertyDefinition[];
}

export interface ScriptSubjectData {
    enabled: boolean;
    properties: Record<string, ScriptPropertyValue>;
}

export interface ObjectSubject {
    readonly id: string;
    readonly kind: 'object';
    readonly data: ObjectSubjectData;
}

export interface TransformSubject {
    readonly id: string;
    readonly kind: 'transform';
    readonly data: TransformSubjectData;
}

export interface ScriptSubject {
    readonly id: string;
    readonly kind: 'script';
    readonly script: ScriptReference | null;
    readonly data: ScriptSubjectData;
}

export type Subject = ObjectSubject | TransformSubject | ScriptSubject;
export type SubjectKind = Subject['kind'];
/** Includes group paths so an entire trait, such as rotation, can be tucked away. */
export type SubjectTraitPath<T> = T extends object
    ? {
        [K in keyof T & string]: K | (T[K] extends object ? `${K}.${SubjectTraitPath<T[K]>}` : never);
    }[keyof T & string]
    : never;

export interface SubjectPresentation<T> {
    readonly inputs: readonly SubjectInputDefinition<T>[];
}

export type SubjectInputType = 'number' | 'string' | 'boolean' | 'vector3' | 'group';

export interface SubjectInput {
    readonly path: string;
    readonly type: SubjectInputType;
    readonly underExpanded?: boolean;
}

type TraitValue<T, P extends string> = P extends `${infer K}.${infer Rest}`
    ? K extends keyof T
        ? TraitValue<T[K], Rest>
        : never
    : P extends keyof T
        ? T[P]
        : never;
type InputType<T> = T extends number
    ? 'number'
    : T extends string
        ? 'string'
        : T extends boolean
            ? 'boolean'
            : T extends Vector3Value
                ? 'vector3'
                : T extends object
                    ? 'group'
                    : never;
export type SubjectInputDefinition<T> = {
    [P in SubjectTraitPath<T>]: {
        readonly path: P;
        readonly type: InputType<TraitValue<T, P>>;
        readonly underExpanded?: boolean;
    };
}[SubjectTraitPath<T>];
export type SubjectUpdate = {
    [K in SubjectKind]: {
        readonly id: string;
        readonly kind: K;
        readonly data: Extract<Subject, { kind: K }>['data'];
    };
}[SubjectKind];

export const SUBJECT_DEFINITIONS = {
    object: {
        title: 'Object',
        icon: 'object',
        inputs: [
            {path: 'name', type: 'string'},
            {path: 'visible', type: 'boolean', underExpanded: true},
        ],
    },
    transform: {
        title: 'Transform',
        icon: 'cube',
        inputs: [
            {path: 'position', type: 'vector3'},
            {path: 'rotation', type: 'vector3'},
            {path: 'scale', type: 'vector3'},
        ],
    },
    script: {title: 'Script', icon: 'script', inputs: [{path: 'enabled', type: 'boolean'}]},
} as const satisfies {
    [K in SubjectKind]: {
    readonly title: string;
    readonly icon: EditorIconName;
} & SubjectPresentation<Extract<Subject, { kind: K }>['data']>;
};

export function defaultTransform(): TransformSubjectData {
    return {
        position: {x: 0, y: 0, z: 0},
        rotation: {x: 0, y: 0, z: 0},
        scale: {x: 1, y: 1, z: 1},
    };
}

export function isVector3(value: unknown): value is Vector3Value {
    if (typeof value !== 'object' || value === null) return false;
    return ['x', 'y', 'z'].every(
        (key) =>
            typeof Reflect.get(value, key) === 'number' && Number.isFinite(Reflect.get(value, key)),
    );
}

export function validScriptValue(definition: ScriptPropertyDefinition, value: unknown): boolean {
    return definition.type === 'vector3'
        ? isVector3(value)
        : definition.type === 'number'
            ? typeof value === 'number' && Number.isFinite(value)
            : typeof value === definition.type;
}
