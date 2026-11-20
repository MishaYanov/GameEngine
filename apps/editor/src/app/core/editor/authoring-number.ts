/** Parse a complete numeric draft without losing nonzero values to f32 underflow. */
export function parseF32(raw: string): number | null {
    if (!/^[+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?$/.test(raw.trim())) return null;
    const value = Number(raw);
    const rounded = Math.fround(value);
    if (!Number.isFinite(value) || !Number.isFinite(rounded)) return null;
    if (value === 0 ? /[1-9]/.test(raw.split(/[eE]/)[0]) : rounded === 0) return null;
    return value;
}
