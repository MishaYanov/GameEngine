import {Component} from '@angular/core';
import {RouterOutlet} from '@angular/router';

@Component({
    selector: 'app-root',
    imports: [RouterOutlet],
    template: '<router-outlet />',
    host: {
        '(window:dragover)': 'preventExternalFileDrop($event)',
        '(window:drop)': 'preventExternalFileDrop($event)',
    },
})
export class App {
    protected preventExternalFileDrop(event: DragEvent): void {
        if (event.dataTransfer?.types.includes('Files')) event.preventDefault();
    }
}
