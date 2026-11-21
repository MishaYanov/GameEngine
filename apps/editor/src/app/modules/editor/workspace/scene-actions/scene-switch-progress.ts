import type {EditorSnapshot, ViewportStatus} from '../../../../core/editor/editor.models';
import type {SceneSwitchService} from './scene-switch.service';

interface SceneSwitchProgressInput {
    readonly switching: ReturnType<SceneSwitchService['loading']>;
    readonly accepted: ReturnType<SceneSwitchService['lastSwitch']>;
    readonly snapshot: Pick<EditorSnapshot, 'active_scene_id' | 'scene_revision'> | null;
    readonly viewport: Pick<ViewportStatus, 'attached' | 'error' | 'scene_load'> | null;
    readonly generation: number;
    readonly nativeGeneration: number | null | undefined;
    readonly gameView: boolean;
    readonly opening: boolean;
}

interface SceneSwitchProgress {
    readonly name: string;
    readonly label: string;
    readonly completed: number | null;
    readonly total: number | null;
}

export function sceneSwitchProgress({
                                        switching,
                                        accepted,
                                        snapshot,
                                        viewport,
                                        generation,
                                        nativeGeneration,
                                        gameView,
                                        opening,
                                    }: SceneSwitchProgressInput): SceneSwitchProgress | null {
    const target = switching ?? accepted;
    if (!target) return null;
    if (!switching && (!accepted || accepted.generation !== generation ||
        snapshot?.active_scene_id !== accepted.sceneId || gameView || opening ||
        !viewport?.attached || viewport.error)) return null;

    const name = target.path.split('/').at(-1) ?? target.path;
    const load = viewport?.scene_load;
    const revision = switching ? snapshot?.scene_revision : accepted?.sceneRevision;
    const matching = load && load.scene_id === target.sceneId && snapshot?.active_scene_id === target.sceneId &&
        revision !== undefined && load.scene_revision >= revision &&
        (nativeGeneration == null || load.workspace_generation === nativeGeneration);
    if (matching && load.stage === 'presented' && !switching) return null;
    if (matching && load.stage === 'preparing' && load.total != null && load.total > 0 && load.completed != null) {
        return {name, label: 'Preparing scene objects…', completed: load.completed, total: load.total};
    }
    const label = matching && load.stage === 'rendering' ? 'Presenting the scene…'
        : switching ? 'Switching scene…' : 'Preparing the scene…';
    return {name, label, completed: null, total: null};
}
