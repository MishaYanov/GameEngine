import {Component} from '@angular/core';
import {LeftDrawer} from "./components/left-drawer/left-drawer";
import {RightDrawer} from "./components/right-drawer/right-drawer";
import {BottomDrawer} from "./components/bottom-drawer/bottom-drawer";
import {TabsManager} from "./components/tabs-manager/tabs-manager";

@Component({
    selector: 'editor-grid-wrapper',
    imports: [
        LeftDrawer,
        RightDrawer,
        BottomDrawer,
        TabsManager
    ],
    templateUrl: './editor-grid-wrapper.html',
    styleUrl: './editor-grid-wrapper.scss',
})
export class EditorGridWrapper {
}
