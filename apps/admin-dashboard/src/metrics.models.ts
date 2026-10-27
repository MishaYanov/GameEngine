export interface ResourceRecord {
    id: number;
    kind: string;
    handle: string;
    label: string;
    details: string;
    payload_bytes: number;
    allocation_bytes: number;
    memory_type: number | null;
    memory_heap: number | null;
    device_local: boolean;
    host_visible: boolean;
    created_ms: number;
    released_ms: number | null;
}

export interface ResourceTotals {
    live_resources: number;
    created_resources: number;
    released_resources: number;
    live_allocations: number;
    allocation_bytes: number;
    peak_allocation_bytes: number;
    allocated_bytes_total: number;
    payload_bytes: number;
    device_local_bytes: number;
    host_visible_bytes: number;
    by_kind: Record<string, number>;
}

export interface ResourceSnapshot {
    schema_version: number;
    device_id: number;
    application: string;
    session_id: string;
    process_id: number;
    gpu_name: string;
    started_ms: number;
    updated_ms: number;
    closed: boolean;
    inventory_truncated: boolean;
    totals: ResourceTotals;
    frame: { frames: number; draw_calls: number; instances: number; triangles: number; gpu_time_ms: number | null };
    resources: ResourceRecord[];
    recent_released: ResourceRecord[];
}

export interface DeviceSummary {
    id: string;
    application: string;
    session_id: string;
    process_id: number;
    gpu_name: string;
    updated_ms: number;
    closed: boolean;
}

export interface MetricsPage {
    devices: DeviceSummary[];
    selected: ResourceSnapshot | null;
    storage_error: string | null;
    truncated: boolean;
}

export function bytes(value: number): string {
    if (value < 1024) return `${value} B`;
    const power = Math.min(Math.floor(Math.log(value) / Math.log(1024)), 4);
    return `${(value / 1024 ** power).toFixed(2)} ${['B', 'KiB', 'MiB', 'GiB', 'TiB'][power]}`;
}

export function deviceState(snapshot: Pick<ResourceSnapshot, 'closed' | 'updated_ms'>, now: number): string {
    return snapshot.closed ? 'CLOSED' : now - snapshot.updated_ms > 5000 ? 'STALE' : 'LIVE';
}

export function sampleMetrics(): MetricsPage {
    const time = Date.now();
    const resources: ResourceRecord[] = [
        {
            id: 1,
            kind: 'buffer',
            handle: '0x100',
            label: 'props/robot.glb · vertices',
            details: 'VERTEX_BUFFER | TRANSFER_DST',
            payload_bytes: 1600000,
            allocation_bytes: 1601536,
            memory_type: 0,
            memory_heap: 0,
            device_local: true,
            host_visible: false,
            created_ms: time - 10000,
            released_ms: null
        },
        {
            id: 2,
            kind: 'texture_image',
            handle: '0x200',
            label: 'props/robot.glb · image 0',
            details: '1024x1024 RGBA8_SRGB',
            payload_bytes: 4194304,
            allocation_bytes: 4259840,
            memory_type: 0,
            memory_heap: 0,
            device_local: true,
            host_visible: false,
            created_ms: time - 10000,
            released_ms: null
        },
        {
            id: 3,
            kind: 'swapchain_image',
            handle: '0x300',
            label: 'Presentation image',
            details: '1280x720 · allocation size unavailable',
            payload_bytes: 0,
            allocation_bytes: 0,
            memory_type: null,
            memory_heap: null,
            device_local: false,
            host_visible: false,
            created_ms: time - 10000,
            released_ms: null
        },
    ];
    return {
        devices: [{
            id: 'game-preview:1',
            application: 'game (sample)',
            session_id: 'game-preview',
            process_id: 18420,
            gpu_name: 'Sample Vulkan GPU',
            updated_ms: time,
            closed: false
        }], selected: {
            schema_version: 1,
            device_id: 1,
            application: 'game (sample)',
            session_id: 'game-preview',
            process_id: 18420,
            gpu_name: 'Sample Vulkan GPU',
            started_ms: time - 10000,
            updated_ms: time,
            closed: false,
            inventory_truncated: false,
            totals: {
                live_resources: 3,
                created_resources: 5,
                released_resources: 2,
                live_allocations: 2,
                allocation_bytes: 5861376,
                peak_allocation_bytes: 10055680,
                allocated_bytes_total: 11655680,
                payload_bytes: 5794304,
                device_local_bytes: 5861376,
                host_visible_bytes: 0,
                by_kind: {buffer: 1, texture_image: 1, swapchain_image: 1}
            },
            frame: {frames: 600, draw_calls: 2, instances: 257, triangles: 12850000, gpu_time_ms: 2.34},
            resources,
            recent_released: [{
                ...resources[0],
                id: 4,
                kind: 'buffer',
                label: 'Upload staging buffer',
                allocation_bytes: 4194304,
                payload_bytes: 4194304,
                device_local: false,
                host_visible: true,
                released_ms: time - 9000
            }],
        }, storage_error: null, truncated: false
    };
}
