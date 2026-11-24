export type SaveFileOutcome = 'written' | 'conflict' | 'failed' | 'blocked';

export interface SaveFileReport {
    readonly path: string;
    readonly outcome: SaveFileOutcome;
    readonly message: string | null;
}

export interface RecoveryOffer {
    readonly token: readonly number[];
    readonly captured_at_ms: number;
    readonly files: readonly string[];
    readonly issue: string | null;
}

export interface RecoveryReview {
    readonly workspace_generation: number;
    readonly revision: number;
    readonly offer: RecoveryOffer | null;
}
