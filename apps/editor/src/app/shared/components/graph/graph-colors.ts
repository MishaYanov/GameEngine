import type {GraphPortType} from './graph.models';

const portColors: Partial<Record<GraphPortType, string>> = {
    float: 'var(--color-graph-float)',
    int: 'var(--color-graph-float)',
    vector2: 'var(--color-graph-vector)',
    vector3: 'var(--color-graph-vector)',
    delta2: 'var(--color-graph-vector)',
    button: 'var(--color-graph-vector)',
    bool: 'var(--color-graph-vector)',
    trigger: 'var(--color-graph-vector)',
    events: 'var(--color-graph-vector)',
    pose: 'var(--color-graph-pose)',
    transition: 'var(--color-text-secondary)',
    rootMotion: 'var(--color-text-secondary)',
};

export function graphPortColor(type: GraphPortType): string | null {
    return portColors[type] ?? null;
}
