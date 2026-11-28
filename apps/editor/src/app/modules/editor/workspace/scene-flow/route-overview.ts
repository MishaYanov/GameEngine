import {Component, computed, input, output, signal} from '@angular/core';
import type {SceneDescriptor, SceneId} from '../../../../core/editor/editor.models';
import type {RouteDraft} from './scene-flow-draft';

const NODE_WIDTH = 140;
const NODE_HEIGHT = 36;

@Component({
    selector: 'editor-route-overview',
    templateUrl: './route-overview.html',
    styleUrl: './route-overview.scss',
})
export class RouteOverview {
    readonly routes = input.required<readonly RouteDraft[]>();
    readonly scenes = input.required<readonly SceneDescriptor[]>();
    readonly selected = input.required<number>();
    readonly routeSelected = output<number>();
    protected readonly filter = signal<SceneId>('');

    protected readonly directions = computed(() => this.routes()
        .map((route, index) => ({route, index}))
        .filter(({route}) => route.from !== '' && route.to !== '' && route.from !== route.to));
    private readonly names = computed(() => new Map(this.scenes().map(scene =>
        [scene.id, scene.asset_path.replace(/\.json$/, '')])));
    protected readonly connectedScenes = computed(() => [...new Set(
        this.directions().flatMap(({route}) => [route.from, route.to]),
    )].sort((left, right) => this.name(left).localeCompare(this.name(right))));
    protected readonly effectiveFilter = computed(() => {
        const requested = this.filter();
        return this.connectedScenes().includes(requested) ? requested : '';
    });
    protected readonly visibleRoutes = computed(() => {
        const filter = this.effectiveFilter();
        return this.directions().filter(({route}) => !filter || route.from === filter || route.to === filter);
    });
    protected readonly activeRoute = computed(() => this.visibleRoutes().some(({index}) => index === this.selected())
        ? this.selected().toString() : '');
    protected readonly diagram = computed(() => {
        const entries = this.visibleRoutes();
        const participating = new Set(entries.flatMap(({route}) => [route.from, route.to]));
        const keys = this.connectedScenes().filter(key => participating.has(key));
        const radius = Math.max(110, keys.length * 31);
        const phase = keys.length === 2 ? 0 : -Math.PI / 2;
        const points = keys.map((id, index) => {
            const angle = phase + 2 * Math.PI * index / keys.length;
            return {id, x: Math.cos(angle) * radius, y: Math.sin(angle) * radius * 0.72};
        });
        if (!points.length) return {width: 0, height: 0, nodes: [], lines: []};
        const minX = Math.min(...points.map(point => point.x));
        const minY = Math.min(...points.map(point => point.y));
        const insetX = NODE_WIDTH / 2 + 16;
        const insetY = NODE_HEIGHT / 2 + 16;
        const nodes = points.map(({id, x, y}) => ({id, x: x - minX + insetX, y: y - minY + insetY}));
        const byId = new Map(nodes.map(node => [node.id, node]));
        const lines = entries.flatMap(({route, index}) => {
            const from = byId.get(route.from);
            const to = byId.get(route.to);
            if (!from || !to) return [];
            const x = to.x - from.x;
            const y = to.y - from.y;
            const length = Math.hypot(x, y);
            const dx = x / length;
            const dy = y / length;
            const inset = Math.min((NODE_WIDTH / 2 + 5) / Math.abs(dx), (NODE_HEIGHT / 2 + 5) / Math.abs(dy));
            return [{
                index, x: from.x + inset * dx - 5 * dy, y: from.y + inset * dy + 5 * dx,
                length: Math.max(0, length - 2 * inset), rotation: Math.atan2(y, x) * 180 / Math.PI,
            }];
        });
        return {
            width: Math.ceil(Math.max(...nodes.map(node => node.x)) + insetX),
            height: Math.ceil(Math.max(...nodes.map(node => node.y)) + insetY),
            nodes,
            lines,
        };
    });

    protected name(id: SceneId): string {
        return this.names().get(id) ?? id;
    }
}
