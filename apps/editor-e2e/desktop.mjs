import {spawn} from 'node:child_process';
import {cp, mkdir, mkdtemp, realpath, stat, writeFile} from 'node:fs/promises';
import {createServer} from 'node:net';
import {dirname, isAbsolute, join, resolve} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

export const packageDirectory = process.env.GAME_ENGINE_EDITOR_E2E_PACKAGE;

async function allocatePort() {
    const server = createServer();
    try {
        await new Promise((listening, failed) => {
            server.once('error', failed);
            server.listen(0, '127.0.0.1', listening);
        });
        return server.address().port;
    } finally {
        if (server.listening) await new Promise(closed => server.close(closed));
    }
}

async function until(probe, label, timeoutMs = 20_000) {
    const deadline = Date.now() + timeoutMs;
    let lastError;
    while (Date.now() < deadline) {
        try {
            const value = await probe();
            if (value) return value;
        } catch (error) {
            lastError = error;
        }
        await delay(100);
    }
    throw new Error('Editor did not reach ' + label, {cause: lastError});
}

async function connectWebView(port, child) {
    const page = await until(async () => {
        if (child.exitCode !== null) throw new Error('Editor exited (' + child.exitCode + ')');
        const response = await fetch('http://127.0.0.1:' + port + '/json/list', {
            redirect: 'error',
            signal: AbortSignal.timeout(1200)
        });
        return (await response.json()).find(candidate => candidate.type === 'page' &&
            new URL(candidate.url).origin === 'http://tauri.localhost');
    }, 'the embedded WebView');
    const endpoint = new URL(page.webSocketDebuggerUrl);
    if (endpoint.protocol !== 'ws:' || endpoint.hostname !== '127.0.0.1' ||
        Number(endpoint.port) !== port || !endpoint.pathname.startsWith('/devtools/page/')) {
        throw new Error('Expected the isolated WebView debugger endpoint');
    }

    const socket = new WebSocket(endpoint);
    await new Promise((connected, failed) => {
        const timer = setTimeout(() => {
            socket.close();
            failed(new Error('Timed out connecting to the Editor WebView'));
        }, 5000);
        socket.addEventListener('open', () => {
            clearTimeout(timer);
            connected();
        }, {once: true});
        socket.addEventListener('error', () => {
            clearTimeout(timer);
            failed(new Error('Could not connect to the Editor WebView'));
        }, {once: true});
    });
    return socket;
}

function visibleClickPoint(selector, label) {
    for (const element of document.querySelectorAll(selector)) {
        if (element.disabled || element.getAttribute('aria-disabled') === 'true' ||
            !element.getClientRects().length ||
            (label !== null && (element.getAttribute('aria-label') || element.textContent.trim()) !== label)) continue;
        const point = () => {
            const rect = element.getBoundingClientRect();
            const y = rect.top + rect.height / 2;
            for (const x of [rect.left + rect.width / 2, rect.left + 4, rect.right - 4]) {
                if (element.contains(document.elementFromPoint(x, y))) return {x, y};
            }
            return null;
        };
        const existing = point();
        if (existing) return existing;
        element.scrollIntoView({block: 'nearest', inline: 'nearest'});
        const revealed = point();
        if (revealed) return revealed;
    }
    return null;
}

class WebView {
    #socket;
    #pending = new Map();
    #nextId = 0;

    constructor(socket) {
        this.#socket = socket;
        socket.addEventListener('message', ({data}) => {
            const message = JSON.parse(data);
            const pending = this.#pending.get(message.id);
            if (!pending) return;
            this.#pending.delete(message.id);
            clearTimeout(pending.timer);
            if (message.error) pending.reject(new Error(message.error.message));
            else pending.resolve(message.result);
        });
        socket.addEventListener('close', () => {
            for (const pending of this.#pending.values()) {
                clearTimeout(pending.timer);
                pending.reject(new Error('The Editor WebView closed'));
            }
            this.#pending.clear();
        });
    }

    send(method, params = {}) {
        if (this.#socket.readyState !== WebSocket.OPEN) return Promise.reject(new Error('The Editor WebView is not connected'));
        const id = ++this.#nextId;
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => {
                this.#pending.delete(id);
                reject(new Error('WebView command timed out: ' + method));
            }, 8000);
            this.#pending.set(id, {resolve, reject, timer});
            this.#socket.send(JSON.stringify({id, method, params}));
        });
    }

    async evaluate(expression) {
        const response = await this.send('Runtime.evaluate', {expression, returnByValue: true, awaitPromise: true});
        if (response.exceptionDetails) {
            throw new Error(response.result.description ?? response.exceptionDetails.text);
        }
        return response.result.value;
    }

    async invoke(command, args = {}) {
        return this.evaluate('window.__TAURI_INTERNALS__.invoke(' + JSON.stringify(command) + ', ' + JSON.stringify(args) + ')');
    }

    waitFor(expression, label) {
        return until(() => this.evaluate(expression), label);
    }

    async resize(width, height) {
        await this.send('Emulation.setDeviceMetricsOverride', {width, height, deviceScaleFactor: 1, mobile: false});
        await this.waitFor('innerWidth === ' + width + ' && innerHeight === ' + height, 'the requested viewport size');
    }

    async click(selector, {label = null, button = 'left'} = {}) {
        const locate = '(' + visibleClickPoint.toString() + ')(' + JSON.stringify(selector) + ', ' + JSON.stringify(label) + ')';
        const point = await this.waitFor('(async () => { const first = ' + locate +
            '; if (!first) return null; await new Promise(requestAnimationFrame); await new Promise(requestAnimationFrame);' +
            'const second = ' + locate + '; return second && first.x === second.x && first.y === second.y ? second : null; })()', label ?? selector);
        await this.send('Input.dispatchMouseEvent', {type: 'mouseMoved', ...point});
        await this.send('Input.dispatchMouseEvent', {type: 'mousePressed', button, clickCount: 1, ...point});
        await this.send('Input.dispatchMouseEvent', {type: 'mouseReleased', button, clickCount: 1, ...point});
    }

    async key(key, extra = {}) {
        await this.send('Input.dispatchKeyEvent', {type: 'keyDown', key, ...extra});
        await this.send('Input.dispatchKeyEvent', {type: 'keyUp', key, ...extra});
    }

    async fill(selector, text) {
        await this.click(selector);
        await this.key('a', {code: 'KeyA', modifiers: 2, windowsVirtualKeyCode: 65});
        if (text) await this.type(text);
        else await this.key('Backspace', {code: 'Backspace', windowsVirtualKeyCode: 8});
    }

    async capture(path) {
        const {data} = await this.send('Page.captureScreenshot', {format: 'png'});
        await writeFile(path, Buffer.from(data, 'base64'));
    }

    async type(text) {
        await this.send('Input.insertText', {text});
    }

    close() {
        this.#socket.close();
    }
}

export async function withEditor(name, action, {environment = {}, openSample = true} = {}) {
    if (process.platform !== 'win32' || !packageDirectory) throw new Error('Choose a Windows Editor package');
    const executable = await realpath(join(resolve(packageDirectory), 'game-engine-editor.exe'));
    if (!(await stat(executable)).isFile()) throw new Error('Expected the packaged Editor executable');
    const packageRoot = dirname(executable);
    const configuredRoot = process.env.GAME_ENGINE_EDITOR_E2E_ROOT;
    if (!configuredRoot || !isAbsolute(configuredRoot)) throw new Error('Set GAME_ENGINE_EDITOR_E2E_ROOT to an absolute test output folder');
    await mkdir(configuredRoot, {recursive: true});
    const root = await mkdtemp(join(await realpath(configuredRoot), name + '-'));
    const project = join(root, 'project');
    const paths = Object.fromEntries(['appdata', 'local', 'temp', 'webview', 'config', 'logs'].map(key => [key, join(root, key)]));
    await Promise.all(Object.values(paths).map(path => mkdir(path)));
    await cp(join(packageRoot, 'release', 'samples', 'cinder-oath'), project, {
        recursive: true, force: false, errorOnExist: true,
    });

    const port = await allocatePort();
    const child = spawn(executable, [], {
        cwd: packageRoot, windowsHide: true, stdio: 'ignore', env: {
            ...process.env,
            ...environment,
            APPDATA: paths.appdata, LOCALAPPDATA: paths.local, TEMP: paths.temp, TMP: paths.temp,
            WEBVIEW2_USER_DATA_FOLDER: paths.webview,
            WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=' + port + ' --remote-debugging-address=127.0.0.1',
            ENGINE_EDITOR_CONFIG_DIR: paths.config, ENGINE_LOG_DIR: paths.logs,
        }
    });
    const exited = new Promise(resolve => {
        child.once('exit', resolve);
        child.once('error', resolve);
    });
    const stop = () => {
        if (child.exitCode === null && child.pid !== undefined) child.kill();
    };
    process.once('exit', stop);
    let view;
    try {
        view = new WebView(await connectWebView(port, child));
        await view.waitFor('document.body.innerText.includes("Open Existing Project")', 'project setup');
        if (openSample) {
            await view.invoke('editor_open_project', {
                manifestPath: join(project, 'project.json'), expectedRevision: null, discardUnsaved: false,
            });
            // Match the setup screen, which replaces its entry after opening a project.
            await view.evaluate('location.replace("/editor"); true');
            await view.waitFor('document.querySelector("editor-workspace") && document.body.innerText.includes("Cinder Oath")',
                'the project workspace');
        }
        await action({view, root, project, pid: child.pid});
    } catch (error) {
        if (view) {
            await Promise.allSettled([
                view.capture(join(root, 'failure.png')),
                view.evaluate('document.body.innerText').then(text => writeFile(join(root, 'failure.txt'), text ?? '')),
            ]);
        }
        throw new Error('Desktop case ' + name + ' failed. Evidence is in ' + root, {cause: error});
    } finally {
        process.removeListener('exit', stop);
        view?.close();
        stop();
        await Promise.race([exited, delay(2500)]);
    }
}

