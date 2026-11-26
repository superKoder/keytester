use crate::parse::KeyPress;

pub const HEADER: &str = "|  Cmd  | Ctrl  |  Opt  | Shift |  Key  | Print |";
pub const SEPARATOR: &str = "| ----- | ----- | ----- | ----- | ----- | ----- |";

fn flag(on: bool, name: &str) -> &str {
    if on { name } else { "" }
}

/// One table row; `note` is appended after the last column if non-empty.
pub fn row(k: &KeyPress, note: &str) -> String {
    let cells = [
        flag(k.cmd, "Cmd"),
        flag(k.ctrl, "Ctrl"),
        flag(k.opt, "Opt"),
        flag(k.shift, "Shift"),
        &k.key,
        &k.print,
    ];
    let mut out = String::from("|");
    for c in cells {
        out.push_str(&format!("{c:^7}|"));
    }
    if !note.is_empty() {
        out.push_str("  ");
        out.push_str(note);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_readme_layout() {
        let k = KeyPress { shift: true, key: "2".into(), print: "@".into(), ..Default::default() };
        assert_eq!(row(&k, ""), "|       |       |       | Shift |   2   |   @   |");

        let k = KeyPress { ctrl: true, key: "C".into(), ..Default::default() };
        assert_eq!(
            row(&k, "(QUITTING!)"),
            "|       | Ctrl  |       |       |   C   |       |  (QUITTING!)"
        );

        let all = KeyPress {
            cmd: true,
            ctrl: true,
            opt: true,
            shift: true,
            key: "Y".into(),
            print: String::new(),
        };
        assert_eq!(row(&all, ""), "|  Cmd  | Ctrl  |  Opt  | Shift |   Y   |       |");
        assert_eq!(row(&KeyPress { key: "Space".into(), ..Default::default() }, "").len(), HEADER.len());
    }
}
