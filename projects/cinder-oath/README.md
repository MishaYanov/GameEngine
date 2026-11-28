# Cinder Oath

A knight enters a sealed vault and faces the Cinder Warden. Close the distance, watch for the Warden's windup, and land
three sword hits to win.

## Play

Run `.\dev.ps1 game` from the engine checkout. In an exported Windows build, launch `game.exe`.

| Control                  | Action                              |
|--------------------------|-------------------------------------|
| Enter                    | Skip the opening                    |
| W, A, S, D               | Move                                |
| Click and move the mouse | Capture the mouse and aim           |
| Left mouse button or F   | Swing the sword                     |
| Esc                      | Open Controls and release the mouse |
| F10                      | Restart                             |

## Edit

Open [project.json](project.json) in the engine editor. [The Oath](assets/scenes/oath.json) contains the knight and the
opening camera move. [The Vault](assets/scenes/vault.json) contains the duelist, the Warden, and the room. Starting Play
runs the project from the opening scene. In the arena, click inside the Game view to capture the mouse and swing.
You can also strike with F. Escape releases the captured mouse in the Editor.

The knight, Warden, and first-person sword each have mesh and animation data in
the [actor assets](assets/actors). [The soundtrack manifest](assets/audio/soundtrack.json) lists the looping chiptune
and the committed combat effects. Player Controls sets their volume.

## Build a Windows copy

From the engine checkout, pass an existing destination directory to the compiler.

~~~powershell
cargo run --offline --locked -p compiler -- --export projects/cinder-oath/project.json 'D:\Game Builds'
~~~

The compiler creates a `Cinder Oath` folder containing the player and its project files.
