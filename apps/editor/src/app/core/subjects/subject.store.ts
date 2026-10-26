import {patchState, signalStore, withMethods, withProps, withState} from '@ngrx/signals';
import {
    isVector3,
    ScriptAsset,
    ScriptSubject,
    Subject,
    SUBJECT_DEFINITIONS,
    SubjectUpdate,
    validScriptValue,
} from './subject.models';

interface SubjectState {
    subjects: readonly Subject[];
    scriptAssets: readonly ScriptAsset[];
}

function validateSubject(subject: Subject): void {
    if (!subject.id.trim()) throw new Error('A subject must have a stable id.');
    switch (subject.kind) {
        case 'object':
            if (typeof subject.data.name !== 'string' || typeof subject.data.visible !== 'boolean')
                throw new Error('Invalid Object subject.');
            break;
        case 'transform':
            if (![subject.data.position, subject.data.rotation, subject.data.scale].every(isVector3))
                throw new Error('Transform values must be finite vectors.');
            break;
        case 'script':
            if (typeof subject.data.enabled !== 'boolean') throw new Error('Invalid Script subject.');
            break;
    }
}

/** Scoped to an inspector. Engine documents and their persistence stay in EditorService. */
export const SubjectStore = signalStore(
    withState<SubjectState>({subjects: [], scriptAssets: []}),
    withProps(() => ({definitions: SUBJECT_DEFINITIONS})),
    withMethods((store) => {
        const scriptSubject = (id: string): ScriptSubject => {
            const subject = store.subjects().find((subject) => subject.id === id);
            if (subject?.kind !== 'script') throw new Error(`Script subject not found: ${id}`);
            return subject;
        };
        const replace = (subject: Subject) =>
            patchState(store, {
                subjects: store
                    .subjects()
                    .map((current) => (current.id === subject.id ? subject : current)),
            });
        return {
            setSubjects(subjects: readonly Subject[]): void {
                if (new Set(subjects.map((subject) => subject.id)).size !== subjects.length)
                    throw new Error('Subject ids must be unique.');
                subjects.forEach(validateSubject);
                patchState(store, {subjects: structuredClone(subjects)});
            },
            setScriptAssets(assets: readonly ScriptAsset[]): void {
                if (new Set(assets.map((asset) => asset.assetId)).size !== assets.length)
                    throw new Error('Script asset ids must be unique.');
                for (const asset of assets) {
                    if (!asset.assetId.trim() || !asset.path.trim() || !asset.className.trim())
                        throw new Error('Invalid script reference.');
                    if (
                        new Set(asset.properties.map((property) => property.name)).size !==
                        asset.properties.length
                    )
                        throw new Error('Script property names must be unique.');
                    for (const property of asset.properties) {
                        if (!property.name.trim() || !validScriptValue(property, property.defaultValue))
                            throw new Error('Invalid script property definition.');
                    }
                }
                const scriptAssets = structuredClone(assets);
                const subjects = store.subjects().map((subject) => {
                    if (subject.kind !== 'script' || !subject.script) return subject;
                    const asset = scriptAssets.find((asset) => asset.assetId === subject.script?.assetId);
                    // Keep missing references recoverable; the card disables unavailable fields.
                    if (!asset) return subject;
                    const properties = Object.fromEntries(
                        asset.properties.map((property) => {
                            const previous = subject.data.properties[property.name];
                            return [
                                property.name,
                                structuredClone(
                                    previous !== undefined && validScriptValue(property, previous)
                                        ? previous
                                        : property.defaultValue,
                                ),
                            ];
                        }),
                    );
                    return {
                        ...subject,
                        script: {assetId: asset.assetId, path: asset.path, className: asset.className},
                        data: {...subject.data, properties},
                    };
                });
                patchState(store, {scriptAssets, subjects});
            },
            updateSubject(update: SubjectUpdate): void {
                const current = store.subjects().find((subject) => subject.id === update.id);
                if (!current || current.kind !== update.kind)
                    throw new Error('Subject type does not match the update.');
                let next: Subject;
                switch (update.kind) {
                    case 'object':
                        next = {id: current.id, kind: 'object', data: update.data};
                        break;
                    case 'transform':
                        next = {id: current.id, kind: 'transform', data: update.data};
                        break;
                    case 'script': {
                        const script = scriptSubject(current.id);
                        const asset = store
                            .scriptAssets()
                            .find((asset) => asset.assetId === script.script?.assetId);
                        const definitions = asset?.properties ?? [];
                        if (
                            Object.keys(update.data.properties).length !== definitions.length ||
                            definitions.some(
                                (property) => !validScriptValue(property, update.data.properties[property.name]),
                            )
                        ) {
                            throw new Error('Script values do not match the attached script definition.');
                        }
                        next = {...script, data: update.data};
                        break;
                    }
                }
                validateSubject(next);
                replace(structuredClone(next));
            },
            attachScript(id: string, assetId: string): void {
                const subject = scriptSubject(id);
                const asset = store.scriptAssets().find((asset) => asset.assetId === assetId);
                if (!asset) throw new Error(`Script asset not found: ${assetId}`);
                if (subject.script?.assetId === assetId) return;
                const properties = Object.fromEntries(
                    asset.properties.map((property) => [
                        property.name,
                        structuredClone(property.defaultValue),
                    ]),
                );
                replace({
                    ...subject,
                    script: {assetId: asset.assetId, path: asset.path, className: asset.className},
                    data: {...subject.data, properties},
                });
            },
            detachScript(id: string): void {
                const subject = scriptSubject(id);
                replace({...subject, script: null, data: {...subject.data, properties: {}}});
            },
            removeSubject(id: string): void {
                patchState(store, {subjects: store.subjects().filter((subject) => subject.id !== id)});
            },
        };
    }),
);
