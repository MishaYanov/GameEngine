export type EditorErrorKind = 'unavailable' | 'not_open' | 'conflict' | 'busy' | 'backend';

export class EditorClientError extends Error {
    constructor(
        readonly kind: EditorErrorKind,
        readonly operation: string,
        message: string,
        override readonly cause?: unknown,
    ) {
        super(message);
        this.name = 'EditorClientError';
    }
}
