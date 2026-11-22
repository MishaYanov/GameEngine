import {cameraDraft, followFromDraft} from './follow-camera';

describe('game camera settings values', () => {
    it('uses shared follow defaults and rejects incomplete, overflowing or vertical views', () => {
        const draft = {...cameraDraft(null, 'player'), mode: 'follow' as const};
        expect(followFromDraft(draft, ['player'])).toEqual({
            target: 'player', offset: [0, 2, 5], look_offset: [0, 0, 0], smoothing_seconds: 0.15,
        });
        expect(() => followFromDraft({...draft, smoothing: '1e-'}, ['player'])).toThrow('Smoothing');
        expect(() => followFromDraft({...draft, offset: {x: '0', y: '2', z: '0'}}, ['player'])).toThrow('viewing direction');
        expect(() => followFromDraft({...draft, offset: {x: '0', y: '2', z: '1e-50'}}, ['player'])).toThrow('32-bit');
        expect(() => followFromDraft(draft, [])).toThrow('target');
        expect(followFromDraft({...draft, mode: 'fixed'}, [])).toBeNull();
    });
});
