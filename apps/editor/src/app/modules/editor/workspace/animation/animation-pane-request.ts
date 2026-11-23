import type {ScopedPaneAsset, ScopedPaneTarget} from '../scoped-pane-request';

export type AnimationPaneRequest = ScopedPaneTarget & {readonly asset: ScopedPaneAsset} & (
    | {readonly library: 'imported'; readonly componentId: string; readonly clipIndex: number}
    | {readonly library: 'authored'; readonly markerId?: string}
);
