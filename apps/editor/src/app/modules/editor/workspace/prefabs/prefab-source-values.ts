import type {
	PrefabSourceDocument,
	PrefabSourceEntity,
} from '../../../../core/editor/linked-prefabs.models';
import type { Transform, Vec3 } from '../../../../core/editor/editor.models';
import { validComponentViews } from '../../../../core/editor/editor-components';
import { parseF32 } from '../../../../core/editor/authoring-number';

export interface SourceEntityDraft {
	name: string;
	parent: string;
	position: { x: string; y: string; z: string };
	rotation: { x: string; y: string; z: string };
	scale: { x: string; y: string; z: string };
}
export interface ReferenceSlotDraft {
	name: string;
	key: string;
}

function record(value: unknown): value is Record<string, unknown> {
	return typeof value === 'object' && value !== null && !Array.isArray(value);
}
function vector(value: unknown): boolean {
	return Array.isArray(value) && value.length === 3 && value.every(Number.isFinite);
}
function sourceEntity(value: unknown): boolean {
	if (!record(value) || !record(value['transform'])) return false;
	const transform = value['transform'];
	return (
		typeof value['key'] === 'string' &&
		typeof value['name'] === 'string' &&
		(value['parent'] == null || typeof value['parent'] === 'string') &&
		['position', 'rotation', 'scale'].every((key) => vector(transform[key])) &&
		validComponentViews(value['components'])
	);
}

export function readPrefabSource(value: unknown): PrefabSourceDocument {
	if (
		!record(value) ||
		value['schema_version'] !== 3 ||
		value['gameplay'] !== null ||
		!record(value['reference_slots']) ||
		Object.keys(value).some(
			(key) => !['schema_version', 'entities', 'gameplay', 'reference_slots'].includes(key),
		) ||
		!Array.isArray(value['entities']) ||
		value['entities'].length < 1 ||
		value['entities'].length > 4096 ||
		!value['entities'].every(sourceEntity)
	) {
		throw new Error('This prefab source uses an unsupported or malformed component schema.');
	}
	const prefab = value as unknown as PrefabSourceDocument;
	if (new Set(prefab.entities.map((entity) => entity.key)).size !== prefab.entities.length)
		throw new Error('Prefab source entity keys are duplicated.');
	slotValues(
		Object.entries(prefab.reference_slots).map(([name, key]) => ({ name, key })),
		prefab.entities,
	);
	return prefab;
}

export function entityDraft(entity?: PrefabSourceEntity): SourceEntityDraft {
	const fields = (value: Vec3) => ({
		x: String(value[0]),
		y: String(value[1]),
		z: String(value[2]),
	});
	return {
		name: entity?.name ?? '',
		parent: entity?.parent ?? '',
		position: fields(entity?.transform.position ?? [0, 0, 0]),
		rotation: fields(entity?.transform.rotation ?? [0, 0, 0]),
		scale: fields(entity?.transform.scale ?? [1, 1, 1]),
	};
}

export function sourceEntityFromDraft(
	draft: SourceEntityDraft,
	entity: PrefabSourceEntity,
	entities: readonly PrefabSourceEntity[],
): PrefabSourceEntity {
	if (
		!draft.name.trim() ||
		/\p{Cc}/u.test(draft.name) ||
		new TextEncoder().encode(draft.name).length > 1024
	)
		throw new Error('Use a visible object name up to 1,024 UTF-8 bytes.');
	const vector = (input: SourceEntityDraft['position']): Vec3 => {
		const x = parseF32(input.x),
			y = parseF32(input.y),
			z = parseF32(input.z);
		if (x === null || y === null || z === null)
			throw new Error('Local transforms must fit finite 32-bit numbers.');
		return [x, y, z];
	};
	const transform: Transform = {
		position: vector(draft.position),
		rotation: vector(draft.rotation),
		scale: vector(draft.scale),
	};
	const parent = draft.parent || null;
	if (parent && !entities.some((candidate) => candidate.key === parent))
		throw new Error('Choose a parent inside this prefab source.');
	const seen = new Set([entity.key]);
	let ancestor = parent;
	while (ancestor) {
		if (seen.has(ancestor)) throw new Error('A prefab parent cannot create a cycle.');
		seen.add(ancestor);
		ancestor = entities.find((candidate) => candidate.key === ancestor)?.parent ?? null;
	}
	return { ...entity, name: draft.name, parent, transform };
}

export function slotValues(
	rows: readonly ReferenceSlotDraft[],
	entities: readonly PrefabSourceEntity[],
): Readonly<Record<string, string>> {
	if (
		rows.length > 128 ||
		rows.some(
			(row) =>
				!/^[A-Za-z0-9_-]{1,64}$/.test(row.name) ||
				typeof row.key !== 'string' ||
				!row.key ||
				entities.some((entity) => entity.key === row.key),
		) ||
		new Set(rows.map((row) => row.name)).size !== rows.length ||
		new Set(rows.map((row) => row.key)).size !== rows.length
	) {
		throw new Error(
			'Use up to 128 unique slot names and external placeholder keys. Slot names use ASCII letters, digits, underscores or hyphens.',
		);
	}
	return Object.fromEntries(rows.map((row) => [row.name, row.key]));
}
