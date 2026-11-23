import {MAX_AUTHORING_NUMBER_LENGTH, parseF32} from './authoring-number';

describe('parseF32', () => {
    it('accepts signed, decimal and scientific notation without prematurely rounding', () => {
        expect(parseF32(' -1.25 ')).toBe(-1.25);
        expect(parseF32('+.125')).toBe(0.125);
        expect(parseF32('2.')).toBe(2);
        expect(parseF32('1.2e-3')).toBe(0.0012);
        expect(parseF32('0e-999')).toBe(0);
    });

    it('rejects incomplete numbers, nonzero underflow, overflow and overlong input', () => {
        for (const text of ['-', '2e', '0x12', '1e-999', '1e-46', '3.5e38']) {
            expect(parseF32(text)).toBeNull();
        }
        expect(parseF32('0'.repeat(MAX_AUTHORING_NUMBER_LENGTH - 1) + '1')).toBe(1);
        expect(parseF32('0'.repeat(MAX_AUTHORING_NUMBER_LENGTH) + '1')).toBeNull();
    });
});
