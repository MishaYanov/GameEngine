# Conditional controller layouts and object scripts

Proposed for the Game Engine editor. Source inspected at commit 932b537.

Use the **Controls** dialog to decide which physical controls produce game actions in each situation. Use **Inspector → Add Component → Script** to decide what one object does with those actions or with events such as a character entering a trigger. The Controller, Controls Graph, Animator, and scripts keep using the same logical action IDs.

## 1. Controls dialog

Put **Controls** beside **File** and **Game** in the editor's top menu. It opens the existing Input Actions editor in a modal with layout and condition editing. The dialog starts on the selected Player Controller's Input Actions asset when one is available, otherwise on the project default. Switching the asset in this dialog changes what you're editing. The Player Controller keeps its explicit asset assignment control.

| Area | Content and behavior |
| --- | --- |
| Header | Input Actions asset picker and the existing keyboard and gamepad monitor. If the project only uses legacy Game Input settings, offer an explicit **Create Input Actions asset** flow and keep the legacy editor available. |
| Layouts, left | A fixed **Default** row and an ordered list of named layouts. **Add Layout**, **Duplicate**, and **Move Up / Down** use stable layout IDs. The order is the priority when two conditions match. |
| Layout editor, right | **Active when** offers **Scene is…** or **Character is grounded / airborne**. The device picker filters the binding table. Each action shows its effective bindings and whether they are inherited, overridden, or disabled. |
| Test condition | Set a preview scene and a simulated grounded/airborne state. Show the winning layout and which action a pressed control would produce. Preview input stays in the dialog. |
| Footer | **Save Layouts** uses the existing revision and asset fingerprint check. **Cancel** preserves the saved map. A stale save leaves the draft visible with a reload choice. Save is disabled during Play. |

On narrow windows, stack the layout list above the bindings. Keep the menu action unavailable while no project is open or the editor is opening a project. On open, focus the asset field or the selected layout. Escape closes a clean dialog and restores focus to **Controls**. A dirty dialog offers Save, Discard, and Keep Editing. Use the editor's existing modal coordination to suspend viewport input during capture. Disconnect clears live controller input and leaves the simulated condition and saved bindings intact.

### The first example

**Default** binds Space and gamepad A to Jump, and F and gamepad X to Attack. **Airborne** is active when the possessed character is airborne. It overrides Jump to be unbound and binds those two Jump controls to Attack. Move and Look inherit Default. Press Jump to leave the ground, release it, and press again to invoke Attack while airborne. A controller with no valid Character Controller target uses the other matching conditions or Default.

### Draft data and validation

Keep action IDs, labels, and value types in one catalog. Existing v1 assets load as Default-only and retain their existing identity and binding digest. In the proposed v2 asset, the existing `actions[].bindings` remain the Default bindings. A conditional layout contributes a list of action overrides. An omitted action inherits Default. An explicit empty binding list unbinds the action for that layout. Evaluate conditional layouts in saved order and use the first matching layout. If a higher-priority row wins, the preview names that row.

This example only lists the affected actions. The rest of the action catalog uses the same existing records.

```json
{
  "schema_version": 2,
  "actions": [
    {
      "id": "jump",
      "label": "Jump",
      "value": "button",
      "bindings": [
        { "kind": "key", "key": "space" },
        { "kind": "gamepad_button", "button": "a" }
      ]
    },
    {
      "id": "attack",
      "label": "Attack",
      "value": "button",
      "bindings": [
        { "kind": "key", "key": "f" },
        { "kind": "gamepad_button", "button": "x" }
      ]
    }
  ],
  "layouts": [
    {
      "id": "airborne",
      "label": "Airborne",
      "when": { "kind": "character_grounded", "value": false },
      "overrides": [
        { "action_id": "jump", "bindings": [] },
        {
          "action_id": "attack",
          "bindings": [
            { "kind": "key", "key": "space" },
            { "kind": "gamepad_button", "button": "a" }
          ]
        }
      ]
    }
  ]
}
```

Native validation checks stable unique IDs, known conditions and actions, valid scene IDs, and binding types. Validate the complete effective binding map for Default and every layout. Multiple layouts can reuse one control. Two actions in the same effective map that claim it show a conflict on both rows and block Save. Start with a proposed limit of 16 conditional layouts within the existing asset size limit. Conditions are typed engine facts. A future script can publish a declared boolean fact that becomes visible at the following fixed step.

Player-local rebinds keep their existing ownership and digest check. For the first conditional release, personal rebinds change entries inherited from Default, and authored overrides specify their own controls. Before accepting a personal rebind, validate all effective layouts and show the exact layout and action if one conflicts. A stale or incompatible stored rebind falls back to the authored bindings for that asset with a diagnostic. Individual conditional-layout rebinds can use layout IDs in a later version.

### How the game switches layouts

1. Keep raw held state and ordered physical edges in the existing bounded input delivery path until the simulation can resolve them.
2. At the start of each fixed step, read the current scene and the possessed character's last committed grounded state. A grounded condition becomes eligible when that character has a committed motion sample. The layout stays constant for that step.
3. Resolve the first matching layout before routing actions through Player Controller and Controls Graph. Prepare effective lookup tables once when loading the asset.
4. Keep held inputs whose bindings are unchanged. Release outgoing logical actions once. A remapped button already held when the layout changes requires a release and a fresh press before producing the new action. A stick may immediately contribute to an axis action. Drop pointer deltas buffered under the outgoing mapping.
5. Deliver short taps and one-shot actions once even when several fixed steps catch up in one frame. Clear appropriate input on focus loss, disconnect, scene replacement, or a change of possession. Condition switches leave the fixed-step clock running.

Use the same resolver in Editor Play, the standalone Player, and the headless replay path. Put layout resolution upstream of Controls Graph. Its Move, Jump, Attack and Animator wiring can stay keyed by action ID as layouts change.

### Files and acceptance

| Work | Files and outcome |
| --- | --- |
| Open the dialog from the top menu | Add the header action in `apps/editor/src/app/modules/editor/components/editor-header/editor-header.{ts,html}`. Route it through `apps/editor/src/app/modules/editor/editor.html` and `apps/editor/src/app/modules/editor/workspace/editor-workspace.ts` to the workspace-owned manager in `apps/editor/src/app/modules/editor/workspace/game-input/game-input-manager.{ts,html}`. Reuse key capture, the input monitor, and **EditorDialog**. |
| Author and save layouts | Extend `apps/editor/src/app/core/game-input/input-actions.models.ts` and `apps/editor/src/app/modules/editor/workspace/game-input/input-actions-editor.{ts,html,scss}`. Continue through `apps/editor-host/src-tauri/src/commands/editor/input_actions.rs` and the validator in `crates/engine-core/src/input/action_map.rs`. Keep compatible v1 reads and point conflicts to the two affected rows. |
| Resolve during Play and export | Route raw samples from `crates/runtime/src/scene_editor/play_input.rs` through `crates/engine-core/src/simulation_session/player_control.rs` and `crates/engine-core/src/simulation_session/input.rs`. Reuse the loader in `crates/bridge/src/input_actions.rs` and the checks in `compiler/src/preflight.rs`. |
| Acceptance | Exercise the menu with keyboard only. Test fallback, overlapping conditions in saved order, v1 imports, and competing bindings. Test the Space/A transition, a short tap, a held unchanged binding, and loss of focus. Replay the same raw trace with different frame grouping in Editor Play, headless, and an exported Player. Observe the same layout changes and logical action edges. |

Resources [Input actions and possession](../../history/adr/0010-input-actions-and-possession.md) ·
[Runtime state and scheduling](../../history/adr/0005-runtime-lifecycle-and-scheduling.md) ·
[Editor modal and viewport ownership](runtime-and-rendering.md#102-modal-behavior).

## 2. Scripts attached to objects

Expose **Inspector → Add Component → Script**. The script picker shows the scripts the project registers, such as **Open Door on Trigger**. Selecting a script creates a component with an **Enabled** toggle, fields generated from its declared settings, and named object pickers. Missing references appear next to the field. Attachments can be duplicated, removed, or edited with the same undoable component flow as other Inspector sections.

This gives authors familiar Unity-style attachment and lifecycle concepts. The script executes on the engine's fixed-step schedule.

**Recommendation** is to expose a registry of distinct scripts and use JSON to save each attachment's declared configuration. At Play, parse the settings into the script's own types and resolve declared references into live, generation-checked handles. A script reads the parts of its owner and references that its descriptor declares. It requests changes through typed engine commands. This keeps a property such as the door's angle editable in the Inspector, keeps the actual Transform authoritative in the engine, and gives each attached copy private runtime state.

| Approach | Use |
| --- | --- |
| Project script registry | First version. Trusted compiled Rust declares named scripts, settings, reference constraints, hooks, and allowed engine commands. Editor, Player, and headless share the registry. |
| JSON for settings and references | Save attachment data in the scene or Prop. Validate once before Play. Use the same metadata to draw Inspector fields and diagnose missing references. |
| Inject the whole object JSON | Useful as an explicit diagnostic snapshot when investigating an object. For gameplay, deserialize only the script's declared settings. Live Transform, component handles, and private state have different lifetimes, and a copied JSON document would drift from them. |
| Standalone script files | A later project feature can plug into the same descriptor, hook and command API. Choose a language and runtime once the team has tested packaging, debugging, execution limits and reload against a representative project. |

Current **Native Behavior** attachments cover a fixed set of built-ins. Project **Behavior Definitions** have their own existing role. Add Script alongside them through the component registry so both existing authoring paths keep working. The generic subject script picker in the Angular design tooling provides UI ideas. The scene component commands and game loop provide the runtime path.

### What gets saved

Store script attachments with the scene entity or Prop source. A stable attachment ID identifies a specific component on that object. The registry identifies the script implementation and engine API version. The attachment separately records the version of its settings. For example, a script attached to a door can use the door as its owner and resolve one explicitly picked sensor.

```json
{
  "id": "door-control",
  "script_id": "project.open_door_on_trigger",
  "enabled": true,
  "config_version": 1,
  "properties": { "open_angle_degrees": 90, "speed_degrees_per_second": 180 },
  "references": {
    "sensor": { "entity": "door-sensor" }
  }
}
```

Declare the angle as a finite number with a range, the rotation speed as a positive finite number, and the sensor as a required entity with a trigger collider. Inspector shows one editable field per setting and filters picker candidates by reference type. Before Play or build, diagnose enabled scripts with unknown IDs, incompatible configuration, or missing references. Keep those records editable in the Inspector so the author can repair them.

Before a Prop is saved, use local object addresses for references within that Prop. On instantiation or duplication, use the engine's existing remapping path to target the matching copied object. External references are explicit slots bound on a scene instance. Each instance has independent script state regardless of shared code or Prop source. Older scene files load with an empty script list. A scene schema update that adds scripts needs compatible reads and explicit rejection of unsupported newer data in earlier Player versions.

### How scripts join the game loop

| Hook | Runs when | Access |
| --- | --- | --- |
| `on_start` | Once when the enabled attachment first participates in an installed simulation | Owner, typed settings, resolved references, freshly initialized script state, command writer. Preparing a scene can validate and allocate state before installation. |
| `on_action` | When a subscribed logical button action is pressed or released after layout resolution | Routed action ID and edge. In the first version, only attachments on the possessed object consume direct player actions. |
| `on_event` | For subscribed trigger or domain events from the preceding committed step | Typed event and live handles. Trigger enter/exit are sufficient for the first project example. |
| `on_fixed_update` | Each fixed step for attached scripts that opt into the tick | Read-only object/component views, tick and fixed delta, per-instance state, command writer. |
| `on_stop` | On stop, despawn, or scene replacement | Teardown reason and private script state. The instance and its handles are then retired. |

Plan hooks after routed input is available and before the existing simulation actions are applied. Within the script phase, use stable object and attachment order, then `on_start`, event and action delivery, and fixed update. Hold script state, consumed input/event cursors, and queued commands in the same candidate step as other gameplay state. A rejected step keeps the last committed state and reports the attachment and hook that failed. Only committed events can feed the next step.

Resolve handles against the active world and generation. Typed reads can cover transform, motion, or component properties. The command writer checks that an effect is declared, the target still exists, and the component accepts the change. For example, a script can ask a door to rotate locally or set an Animator parameter. Movement on a motor-owned actor goes through the character-controller intent. Keep existing action budgets and add bounds on attachments, config size, and subscribed deliveries. Compiled hooks run as trusted project code. A future untrusted execution backend needs its own CPU and memory controls.

### Proposed Rust hook shape

```rust
// API sketch. The registry adapts typed implementations into prepared instances.
trait ObjectScript {
    type Config;
    type State: Clone;

    fn on_start(
        &self, ctx: &ScriptContext<'_, Self::Config>, state: &mut Self::State,
        out: &mut ScriptCommands<'_>,
    ) -> Result<(), ScriptError>;

    fn on_event(
        &self, ctx: &ScriptContext<'_, Self::Config>, state: &mut Self::State,
        event: &ObjectEvent, out: &mut ScriptCommands<'_>,
    ) -> Result<(), ScriptError>;

    fn on_action(
        &self, _ctx: &ScriptContext<'_, Self::Config>, _state: &mut Self::State,
        _action: &ActionEdge, _out: &mut ScriptCommands<'_>,
    ) -> Result<(), ScriptError> { Ok(()) }

    fn on_fixed_update(
        &self,
        ctx: &ScriptContext<'_, Self::Config>,
        state: &mut Self::State,
        out: &mut ScriptCommands<'_>,
    ) -> Result<(), ScriptError>;

    fn on_stop(&self, _state: &mut Self::State, _reason: StopReason) {}
}

impl ObjectScript for OpenDoorOnTrigger {
    type Config = DoorConfig;
    type State = DoorState;

    fn on_start(
        &self, ctx: &ScriptContext<'_, DoorConfig>, state: &mut DoorState,
        _out: &mut ScriptCommands<'_>,
    ) -> Result<(), ScriptError> {
        state.closed_yaw = ctx.transform().local_yaw_degrees();
        Ok(())
    }

    fn on_event(
        &self, ctx: &ScriptContext<'_, DoorConfig>, state: &mut DoorState,
        event: &ObjectEvent, _out: &mut ScriptCommands<'_>,
    ) -> Result<(), ScriptError> {
        if event.entered_trigger(ctx.references().sensor()) {
            state.open = true;
        }
        Ok(())
    }

    fn on_fixed_update(
        &self,
        ctx: &ScriptContext<'_, DoorConfig>,
        state: &mut DoorState,
        out: &mut ScriptCommands<'_>,
    ) -> Result<(), ScriptError> {
        if state.open {
            let current = ctx.transform().local_yaw_degrees();
            let goal = state.closed_yaw + ctx.config().open_angle_degrees;
            let step = ctx.config().speed_degrees_per_second * ctx.step_seconds();
            let next = approach_angle(current, goal, step);
            if next != current {
                out.set_local_yaw_degrees(ctx.owner(), next)?;
            }
        }
        Ok(())
    }
}
```

The other lifecycle hooks can default to doing nothing. The adapter validates and prepares `DoorConfig` once, keeps `DoorState` per instance, and translates commands into the current simulation pipeline. The example rotates the door through an engine-controlled motion over time. Restart creates fresh script instances. The first version applies script configuration edits on the next Play and takes code changes from the rebuilt trusted project.

### Files and acceptance

| Work | Files and outcome |
| --- | --- |
| Add Script to object authoring | Extend `crates/engine-core/src/components.rs`, `crates/engine-core/src/components/entity.rs` and `crates/engine-core/src/scene_data.rs`. Use a dedicated `crates/engine-core/src/components/script.rs` for proposed settings, descriptors and typed references. Extend `apps/editor/src/app/core/editor/editor-components.ts` and `apps/editor/src/app/modules/editor/workspace/components/scene-component-inspector.ts`. Reuse the typed component operations in `crates/engine-core/src/editor/operations.rs`. |
| Schedule script instances | Add a script adapter under `crates/engine-core/src/simulation_session/` and wire it into the existing tick in `crates/engine-core/src/simulation_session.rs`. Validate handles through `crates/engine-core/src/runtime_lifecycle.rs` and route commands through the bounded writer in `crates/engine-core/src/simulation_session/gameplay.rs`. |
| Register and verify project scripts | Follow the injected registry pattern in `crates/gameplay-catalog/src/lib.rs`. Supply the shared registry to `apps/editor-host/src-tauri/src/editor.rs`, `apps/game/src/main.rs` and `apps/headless/src/replay.rs`. Check the script ID, API version, config, reference constraints and declared asset dependencies in `compiler/src/preflight.rs`. Put the sample implementation in project gameplay code. |
| Acceptance | Attach Open Door on Trigger, assign its sensor, and enter it in Play. Save, reopen, and verify the same setup in the standalone Player and headless. Duplicate a Prop with its own sensor, then activate only one copy. Test invalid and stale refs, independent state, invalid commands leaving the last committed state intact, Stop, and Restart. |

Resources [Game modules and scripting](../../history/adr/0008-game-modules-and-scripting.md) ·
[Object composition and components](../../history/adr/0002-object-composition-and-components.md) ·
[Authoring and Play ownership](../../history/adr/0004-authoring-play-and-editor-state.md) ·
[Typed Rust and TypeScript bridge](../../history/adr/0003-rust-typescript-bridge.md).

## First two working examples

1. **Airborne controls.** Use the Controls dialog to create one conditional layout for the existing Player Controller. Verify that one button jumps from the ground and a fresh press invokes Attack after the layout switches.
2. **Two trigger doors.** Add one registered script through the Inspector. Configure the door and duplicate it with its sensor as a Prop. Play the scene and verify independent effects on each copy. This establishes the script contract before extending the editor to arbitrary script files.
