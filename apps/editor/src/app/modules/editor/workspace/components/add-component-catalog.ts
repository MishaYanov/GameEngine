import type {ComponentData, ComponentRecord, ComponentTypeDescriptor} from '../../../../core/editor/editor-components';

export type SpecialistAttachment = 'animator' | 'controls_graph';
type ComponentKind = ComponentData['kind'];
type ComponentGroup = 'Rendering' | 'Physics' | 'Gameplay' | 'Audio' | 'Animation';

export interface AddComponentChoice {
    readonly id: ComponentKind | SpecialistAttachment;
    readonly label: string;
    readonly group: ComponentGroup;
    readonly description: string;
    readonly keywords: string;
    readonly unavailableReason?: string;
}

export interface SpecialistAttachmentChoice {
    readonly id: SpecialistAttachment;
    readonly unavailableReason?: string;
}

const capacity = 16;
const groups: Partial<Record<ComponentKind | SpecialistAttachment, ComponentGroup>> = {
    mesh_renderer: 'Rendering', light: 'Rendering', sky: 'Rendering', particle_emitter: 'Rendering',
    collider: 'Physics', character_controller: 'Physics',
    audio_source: 'Audio', animation: 'Animation', animator: 'Animation',
};
const specialists = {
    animator: {label: 'Animator', description: 'Attach an Animator graph to this object.'},
    controls_graph: {label: 'Controls Graph', description: 'Choose how this Player Controller responds to input.'},
} as const;

export function addComponentCatalog(
    types: readonly ComponentTypeDescriptor[],
    attachments: readonly ComponentRecord[],
    pinned: (kind: ComponentKind) => boolean,
    specialized: readonly SpecialistAttachmentChoice[] = [],
    objectCapacity = attachments.length,
): readonly AddComponentChoice[] {
    const counts = new Map<ComponentKind, number>();
    for (const attachment of attachments) {
        counts.set(attachment.data.kind, (counts.get(attachment.data.kind) ?? 0) + 1);
    }
    const choices = types.map<AddComponentChoice>(type => ({
        id: type.kind,
        label: type.label,
        group: groups[type.kind] ?? 'Gameplay',
        keywords: type.type_id,
        description: type.kind === 'collider' ? 'Add a Box or Sphere collider in local space.' : type.description,
        unavailableReason: pinned(type.kind) ? 'Edit the Prop source to attach this component.'
            : objectCapacity >= capacity ? 'This object already has 16 components.'
            : (counts.get(type.kind) ?? 0) >= type.max_per_object ? 'This object already has its maximum for this type.'
            : undefined,
    }));
    for (const attachment of specialized) {
        const {id, unavailableReason} = attachment;
        if (choices.some(choice => choice.id === id)) continue;
        choices.push({
            id, ...specialists[id], group: groups[id] ?? 'Gameplay', keywords: 'graph',
            unavailableReason: unavailableReason ??
                (id === 'animator' && objectCapacity >= capacity ? 'This object already has 16 components.' : undefined),
        });
    }
    return choices.sort((first, second) => first.group.localeCompare(second.group) || first.label.localeCompare(second.label));
}
