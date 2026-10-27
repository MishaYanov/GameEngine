import {describe, expect, it} from 'vitest';
import {bytes, deviceState, sampleMetrics} from './metrics.models';

describe('Resource metrics presentation', () => {
    it('keeps missing GPU timing distinct from zero and classifies stale and closed sessions', () => {
        expect(deviceState({closed: false, updated_ms: 1000}, 2000)).toBe('LIVE');
        expect(deviceState({closed: false, updated_ms: 1000}, 7000)).toBe('STALE');
        expect(deviceState({closed: true, updated_ms: 1000}, 2000)).toBe('CLOSED');
        expect(bytes(0)).toBe('0 B');
        expect(bytes(1024 * 1024)).toBe('1.00 MiB');
    });
    it('keeps presentation images outside exact allocated-byte totals in preview', () => {
        const snapshot = sampleMetrics().selected!;
        expect(snapshot.resources.reduce((sum, row) => sum + row.allocation_bytes, 0)).toBe(snapshot.totals.allocation_bytes);
        expect(snapshot.resources.find(row => row.kind === 'swapchain_image')?.memory_type).toBeNull();
        expect(snapshot.totals.allocation_bytes).toBeGreaterThan(snapshot.totals.payload_bytes);
    });
});
