# Windows Editor desktop tests

These tests launch the portable Windows Editor, copy its packaged Cinder Oath sample, and use an isolated WebView2
profile. They verify the embedded production frontend and the real editor runtime. The source sample, user projects, and
normal Editor profile are outside the fixture.

Build the portable Editor with the repository's Windows release script. In PowerShell from the repository, set the two
paths and run the ordinary suite serially.

```powershell
$env:GAME_ENGINE_EDITOR_E2E_PACKAGE = 'D:\Builds\editor\Authoring Release 2\Editor'
$env:GAME_ENGINE_EDITOR_E2E_ROOT = 'D:\Editor QA\authoring'
node.exe --test --test-concurrency=1 apps/editor-e2e/authoring.test.mjs apps/editor-e2e/native-viewport.test.mjs
```

The Node.js runtime must provide built-in Fetch, WebSocket, and the native test runner. The native pointer case uses
Python 3. Set `GAME_ENGINE_EDITOR_E2E_PYTHON` if Python is outside PATH. It sends Win32 client-coordinate messages to
the validated fixture viewport and checks its native selection, one-axis movement, and Undo. An SDK is optional. Set
`GAME_ENGINE_EDITOR_E2E_VULKAN_LAYER_PATH` to the installed Vulkan SDK Bin directory containing
`VkLayer_screenshot.json` to also inspect actual renderer pixels for occlusion and grid depth.

Most authoring operations use mouse and keyboard events delivered to the WebView. Native calls read state and prepare
fixtures. The standalone export test supplies its test directory directly to the packaged build command. The Angular
workspace test checks that the menu opens the Windows folder chooser. The desktop test verifies the output executable
and a soundtrack by hash.

For longer verification use the same package and isolated output root.

```powershell
$env:GAME_ENGINE_EDITOR_E2E_SOAK_SECONDS = '600'
node.exe --test --test-concurrency=1 apps/editor-e2e/authoring-soak.test.mjs
```

One fixture repeats two scene transitions, asset search, native Animation preview, and Play/Stop at four window sizes.
Its `cycles.jsonl` tracks elapsed time, confirmed switch times, and samples of editor process working set and WebView
JavaScript heap. The native working set excludes the browser and GPU process. The same fixture verifies that its
authored scene document stays unchanged.

The fixture directory contains its project copy and native logs. Visual cases save selected evidence. Failures also
attempt to save a WebView screenshot and text snapshot.

