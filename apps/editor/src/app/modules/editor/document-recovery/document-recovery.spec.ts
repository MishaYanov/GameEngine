import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import {EditorService} from '../../../core/editor/editor.service';
import {EditorDocumentRecovery} from './document-recovery';

describe('Editor draft recovery', () => {
    it('reviews once per open workspace without tracking command queue activity', async () => {
        const generation = signal(1);
        const pending = signal(0);
        const reviewDocumentRecovery = vi.fn(async (current: number) => {
            pending.set(pending() + 1);
            return {workspace_generation: current, revision: 0, offer: null};
        });
        TestBed.configureTestingModule({providers: [{
            provide: EditorService,
            useValue: {
                snapshot: () => ({dirty: false}),
                workspaceGeneration: generation,
                capabilities: () => ({document_recovery_supported: true}),
                reviewDocumentRecovery,
            },
        }]});
        const fixture = TestBed.createComponent(EditorDocumentRecovery);
        await fixture.whenStable();
        expect(reviewDocumentRecovery.mock.calls).toEqual([[1]]);

        pending.set(0);
        await fixture.whenStable();
        expect(reviewDocumentRecovery.mock.calls).toEqual([[1]]);

        generation.set(2);
        await fixture.whenStable();
        expect(reviewDocumentRecovery.mock.calls).toEqual([[1], [2]]);
        fixture.destroy();
    });
});
