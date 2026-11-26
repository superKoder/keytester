# keytester

Simple command line program, that shows your keystrokes of your (fully
programmable) keyboard.

Example output:

```cmd
$ ./keytester

|  Cmd  | Ctrl  |  Opt  | Shift |  Key  | Print |
| ----- | ----- | ----- | ----- | ----- | ----- |
|       |       |       |       |   A   |   b   |
|       |       |       | Shift |   B   |   B   |
|       |       |       |       |   C   |   c   |
|       |       |       |       | Space |       |
|       |       |       |       |  Esc  |       |  (press 3 times to quit)
|  Cmd  |       |       |       |   X   |       |
|  Cmd  | Ctrl  |  Opt  | Shift |   Y   |       |
|       |       |       | Shift |   2   |   @   |
|  Cmd  | Ctrl  |       |       |   C   |       |  (press 3 times to quit)
|       | Ctrl  |       |       |   C   |       |  (press 3 times to quit)
|       | Ctrl  |       |       |   C   |       |  (QUITTING!)
```

## Build & run

```sh
cargo build --release
./target/release/keytester
```

Press `Esc` or `Ctrl+C` three times in a row to quit.

## Terminal support

Full detail (Cmd, and _Key_ vs. _Print_ such as `2` → `@`) needs a terminal with the
[kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/): kitty, Ghostty,
WezTerm, or iTerm2 (enable _Report keys using CSI u_).

Other terminals (e.g. Terminal.app) run in a legacy mode that can't see Cmd or
the unshifted key. Shortcuts the terminal itself binds (like Cmd+C/Cmd+V) may
never reach the program.
