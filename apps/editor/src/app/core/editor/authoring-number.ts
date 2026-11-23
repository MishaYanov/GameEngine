export const MAX_AUTHORING_NUMBER_LENGTH = 128;

/** Parse a complete numeric draft without losing nonzero values to f32 underflow. */
export function parseF32(raw: string): number | null {
    if (raw.length > MAX_AUTHORING_NUMBER_LENGTH) return null;
    const text = raw.trim();
    if (!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(text)) return null;
    const value = Number(text);
    const rounded = Math.fround(value);
    if (!Number.isFinite(value) || !Number.isFinite(rounded)) return null;
    if (value === 0 ? /[1-9]/.test(text.split(/[eE]/)[0]) : rounded === 0) return null;
    return value;
}
