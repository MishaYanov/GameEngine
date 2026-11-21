import type {SearchPickerOption} from '../../../shared/components/search-picker/search-picker';

interface EntityReference {
    readonly key: string;
    readonly name: string;
    readonly parent?: string | null;
}

export function sceneReferenceOptions(entities: readonly EntityReference[]): readonly SearchPickerOption[] {
    const byKey = new Map(entities.map(entity => [entity.key, entity]));
    const locations = entities.map(entity => {
        const ancestors: string[] = [];
        const seen = new Set([entity.key]);
        let parent = entity.parent;
        while (parent && !seen.has(parent)) {
            seen.add(parent);
            const ancestor = byKey.get(parent);
            if (!ancestor) break;
            ancestors.push(ancestor.name);
            parent = ancestor.parent;
        }
        return {
            entity,
            location: ancestors.reverse().join(' / ') || 'Scene root',
        };
    });
    const names = new Map<string, number>();
    const nameAtLocation = ({entity, location}: typeof locations[number]) => JSON.stringify([location, entity.name]);
    for (const entry of locations) {
        const name = nameAtLocation(entry);
        names.set(name, (names.get(name) ?? 0) + 1);
    }
    return locations.map(entry => ({
        id: entry.entity.key,
        label: entry.entity.name,
        description: names.get(nameAtLocation(entry)) === 1
            ? entry.location
            : `${entry.location} · ${entry.entity.key}`,
    }));
}
