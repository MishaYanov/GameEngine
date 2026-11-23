import {validAudioIdentity} from '../../../../core/editor/audio-source.models';

export const audioClipDragType = 'application/x-game-engine-audio-clip';

export function startAudioClipDrag(event: DragEvent, id: string, generation: number): void {
    const transfer = event.dataTransfer;
    if (!transfer || !validAudioIdentity(id) || !Number.isSafeInteger(generation)) return;
    transfer.effectAllowed = 'copy';
    transfer.setData(audioClipDragType, `${generation}:${id}`);
}

export function readAudioClipDrag(transfer: DataTransfer | null, generation: number | null): string | null {
    if (!transfer || generation === null || !transfer.types.includes(audioClipDragType)) return null;
    const value = transfer.getData(audioClipDragType);
    const prefix = `${generation}:`;
    if (!value.startsWith(prefix)) return null;
    const id = value.slice(prefix.length);
    return validAudioIdentity(id) ? id : null;
}
