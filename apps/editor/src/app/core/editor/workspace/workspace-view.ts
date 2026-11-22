/** Content open in the center of the editor. The editor session owns the active scene and play state. */
export type WorkspaceView =
    | { readonly kind: 'scene-viewport' }
    | { readonly kind: 'game-preview' }
    | { readonly kind: 'prop-workspace' }
    | { readonly kind: 'animation-workspace' }
    | { readonly kind: 'animation-controller'; readonly assetId: string }
    | { readonly kind: 'shader-graph'; readonly assetId: string };

export type WorkspaceViewKind = WorkspaceView['kind'];
