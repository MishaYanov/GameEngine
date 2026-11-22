import {Component, input} from '@angular/core';
import {type FieldTree, FormField} from '@angular/forms/signals';
import {EditorSearchPicker, type SearchPickerOption} from '../../../../shared/components/search-picker/search-picker';
import type {GameCameraDraft} from './camera-draft';

@Component({
    selector: 'editor-camera-rig-fields',
    imports: [FormField, EditorSearchPicker],
    templateUrl: './camera-rig-fields.html',
    styleUrl: './settings.scss',
})
export class CameraRigFields {
    readonly draft = input.required<GameCameraDraft>();
    readonly fields = input.required<FieldTree<GameCameraDraft>>();
    readonly targets = input.required<readonly SearchPickerOption[]>();
    readonly disabled = input(false);
    readonly focusTarget = input(false);
    protected readonly axes = ['x', 'y', 'z'] as const;
    protected readonly groups = [
        {name: 'offset', label: 'Camera offset'},
        {name: 'look_offset', label: 'Look offset'},
    ] as const;
    protected readonly orbitNumbers = [
        {key: 'yawDegrees', label: 'Initial yaw in degrees'},
        {key: 'pitchDegrees', label: 'Initial pitch in degrees'},
        {key: 'minimumPitchDegrees', label: 'Minimum pitch in degrees'},
        {key: 'maximumPitchDegrees', label: 'Maximum pitch in degrees'},
        {key: 'distance', label: 'Initial distance'},
        {key: 'minimumDistance', label: 'Minimum distance'},
        {key: 'maximumDistance', label: 'Maximum distance'},
        {key: 'followSeconds', label: 'Follow response in seconds'},
        {key: 'recoverySeconds', label: 'Outward recovery in seconds'},
        {key: 'teleportDistance', label: 'Teleport snap distance'},
        {key: 'probeRadius', label: 'Probe radius'},
        {key: 'obstructionMask', label: 'Obstacle layer mask'},
    ] as const;
}
