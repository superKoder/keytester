//! Turns raw terminal input bytes into key presses.
//!
//! Handles both the kitty keyboard protocol (`CSI code:shifted:base ; mods:event ; text u`)
//! and the legacy encodings (control bytes, ESC-prefixed Alt, xterm CSI/SS3 sequences).

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyPress {
    pub cmd: bool,
    pub ctrl: bool,
    pub opt: bool,
    pub shift: bool,
    /// The key that was hit (base layout key), e.g. `2`, `A`, `Esc`.
    pub key: String,
    /// The text that key produced, e.g. `@`. Empty if nothing printable.
    pub print: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    /// A key press was decoded.
    Key(KeyPress),
    /// Bytes were consumed but produce no row (release events, lone modifiers, terminal replies).
    Ignored,
    /// The buffer ends in the middle of a sequence; wait for more bytes.
    Incomplete,
}

const ESC: u8 = 0x1b;

/// Parses one event from the start of `buf`. Returns the result and the number of bytes consumed
/// (0 for `Incomplete`). `buf` must not be empty.
pub fn parse_one(buf: &[u8]) -> (Parsed, usize) {
    debug_assert!(!buf.is_empty());
    if buf[0] != ESC {
        return parse_plain(buf);
    }
    match buf.get(1) {
        None => (Parsed::Incomplete, 0),
        Some(b'[') => parse_csi(buf),
        Some(b'O') => parse_ss3(buf),
        Some(_) => {
            // ESC + key: legacy encoding of Opt+key.
            let (parsed, n) = parse_one(&buf[1..]);
            match parsed {
                Parsed::Key(mut k) => {
                    k.opt = true;
                    k.print.clear();
                    (Parsed::Key(k), n + 1)
                }
                Parsed::Incomplete => (Parsed::Incomplete, 0),
                Parsed::Ignored => (Parsed::Ignored, n + 1),
            }
        }
    }
}

/// Called when no more input arrives: a pending lone ESC is the Esc key.
pub fn flush(buf: &[u8]) -> Option<KeyPress> {
    (buf == [ESC]).then(|| named("Esc"))
}

fn named(key: &str) -> KeyPress {
    KeyPress { key: key.to_string(), ..Default::default() }
}

fn ctrl(key: &str) -> KeyPress {
    KeyPress { ctrl: true, key: key.to_string(), ..Default::default() }
}

/// Single (non-escape) byte or UTF-8 character, legacy mode.
fn parse_plain(buf: &[u8]) -> (Parsed, usize) {
    let b = buf[0];
    let key = match b {
        b'\r' => named("Enter"),
        b'\t' => named("Tab"),
        0x7f => named("Bksp"),
        0x00 => ctrl("Space"),
        0x01..=0x1a => ctrl(&((b'A' + b - 1) as char).to_string()),
        0x1c => ctrl("\\"),
        0x1d => ctrl("]"),
        0x1e => ctrl("^"),
        0x1f => ctrl("_"),
        _ => {
            let len = utf8_len(b);
            if buf.len() < len {
                return (Parsed::Incomplete, 0);
            }
            let Ok(s) = std::str::from_utf8(&buf[..len]) else {
                return (Parsed::Ignored, 1);
            };
            let c = s.chars().next().unwrap();
            let key = KeyPress {
                shift: c.is_uppercase(),
                key: if c == ' ' { "Space".into() } else { c.to_uppercase().collect() },
                print: s.to_string(),
                ..Default::default()
            };
            return (Parsed::Key(key), len);
        }
    };
    (Parsed::Key(key), 1)
}

fn utf8_len(first: u8) -> usize {
    match first {
        0xf0..=0xf7 => 4,
        0xe0..=0xef => 3,
        0xc0..=0xdf => 2,
        _ => 1,
    }
}

fn parse_ss3(buf: &[u8]) -> (Parsed, usize) {
    let Some(&fin) = buf.get(2) else {
        return (Parsed::Incomplete, 0);
    };
    match final_key(fin) {
        Some(name) => (Parsed::Key(named(name)), 3),
        None => (Parsed::Ignored, 3),
    }
}

/// Keys encoded by the final byte of `CSI 1;mods X` / `SS3 X`.
fn final_key(fin: u8) -> Option<&'static str> {
    Some(match fin {
        b'A' => "Up",
        b'B' => "Down",
        b'C' => "Right",
        b'D' => "Left",
        b'E' => "KP5",
        b'H' => "Home",
        b'F' => "End",
        b'P' => "F1",
        b'Q' => "F2",
        b'R' => "F3",
        b'S' => "F4",
        _ => return None,
    })
}

/// Keys encoded as `CSI num;mods ~`.
fn tilde_key(num: u32) -> Option<&'static str> {
    Some(match num {
        1 | 7 => "Home",
        2 => "Ins",
        3 => "Del",
        4 | 8 => "End",
        5 => "PgUp",
        6 => "PgDn",
        11 => "F1",
        12 => "F2",
        13 => "F3",
        14 => "F4",
        15 => "F5",
        17 => "F6",
        18 => "F7",
        19 => "F8",
        20 => "F9",
        21 => "F10",
        23 => "F11",
        24 => "F12",
        _ => return None,
    })
}

fn parse_csi(buf: &[u8]) -> (Parsed, usize) {
    // ESC [ params(0x30-0x3f)* intermediates(0x20-0x2f)* final(0x40-0x7e)
    let mut i = 2;
    while i < buf.len() && (0x20..=0x3f).contains(&buf[i]) {
        i += 1;
    }
    let Some(&fin) = buf.get(i) else {
        return (Parsed::Incomplete, 0);
    };
    let len = i + 1;
    if !(0x40..=0x7e).contains(&fin) {
        return (Parsed::Ignored, len);
    }
    let params = std::str::from_utf8(&buf[2..i]).unwrap_or("");
    // Private replies such as `CSI ? flags u` or the DA reply `CSI ? ... c`.
    if params.starts_with(['?', '>', '<', '=']) {
        return (Parsed::Ignored, len);
    }

    // fields[n][m]: `;` separates fields, `:` separates sub-fields.
    let fields: Vec<Vec<Option<u32>>> = params
        .split(';')
        .map(|f| f.split(':').map(|s| s.parse().ok()).collect())
        .collect();
    let field = |n: usize, m: usize| fields.get(n).and_then(|f| f.get(m)).copied().flatten();

    let mods = field(1, 0).unwrap_or(1).saturating_sub(1);
    let event = field(1, 1).unwrap_or(1);
    if event == 3 {
        return (Parsed::Ignored, len); // key release
    }

    let mut key = match fin {
        b'u' => {
            let Some(code) = field(0, 0) else {
                return (Parsed::Ignored, len);
            };
            if (57441..=57452).contains(&code) {
                return (Parsed::Ignored, len); // lone modifier key
            }
            let base = field(0, 2).unwrap_or(code);
            let text: String = fields
                .get(2)
                .map(|f| f.iter().flatten().filter_map(|&c| char::from_u32(c)).collect())
                .unwrap_or_default();
            KeyPress {
                key: kitty_key_name(base),
                print: text.chars().filter(|c| !c.is_control()).collect(),
                ..Default::default()
            }
        }
        b'~' => match field(0, 0).and_then(tilde_key) {
            Some(name) => named(name),
            None => return (Parsed::Ignored, len),
        },
        b'Z' => KeyPress { shift: true, ..named("Tab") },
        _ => match final_key(fin) {
            Some(name) => named(name),
            None => return (Parsed::Ignored, len),
        },
    };
    key.shift |= mods & 1 != 0;
    key.opt |= mods & 2 != 0;
    key.ctrl |= mods & 4 != 0;
    key.cmd |= mods & 8 != 0;
    (Parsed::Key(key), len)
}

/// Name for a kitty protocol key code (a Unicode codepoint or a private-use functional key).
fn kitty_key_name(code: u32) -> String {
    let name = match code {
        9 => "Tab",
        13 => "Enter",
        27 => "Esc",
        32 => "Space",
        127 => "Bksp",
        57358 => "CapsL",
        57359 => "ScrL",
        57360 => "NumL",
        57361 => "PrtSc",
        57362 => "Pause",
        57363 => "Menu",
        57376..=57398 => return format!("F{}", code - 57376 + 13),
        57399..=57408 => return format!("KP{}", code - 57399),
        57409 => "KP.",
        57410 => "KP/",
        57411 => "KP*",
        57412 => "KP-",
        57413 => "KP+",
        57414 => "KPEnt",
        57415 => "KP=",
        57416 => "KP,",
        57417 => "KPLft",
        57418 => "KPRgt",
        57419 => "KPUp",
        57420 => "KPDn",
        57421 => "KPPgU",
        57422 => "KPPgD",
        57423 => "KPHom",
        57424 => "KPEnd",
        57425 => "KPIns",
        57426 => "KPDel",
        57427 => "KP5",
        57428 => "Play",
        57429 => "Pause",
        57430 => "PlPau",
        57431 => "Rev",
        57432 => "Stop",
        57433 => "FFwd",
        57434 => "Rew",
        57435 => "Next",
        57436 => "Prev",
        57437 => "Rec",
        57438 => "VolDn",
        57439 => "VolUp",
        57440 => "Mute",
        57453 => "ISO3",
        57454 => "ISO5",
        _ => {
            return match char::from_u32(code) {
                Some(c) if !c.is_control() => c.to_uppercase().collect(),
                _ => format!("#{code}"),
            };
        }
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(bytes: &[u8]) -> KeyPress {
        match parse_one(bytes) {
            (Parsed::Key(k), n) => {
                assert_eq!(n, bytes.len(), "should consume whole sequence");
                k
            }
            other => panic!("expected key, got {other:?}"),
        }
    }

    #[test]
    fn kitty_shift_2_prints_at() {
        let k = key(b"\x1b[50:64:50;2;64u");
        assert!(k.shift && !k.ctrl && !k.cmd && !k.opt);
        assert_eq!((k.key.as_str(), k.print.as_str()), ("2", "@"));
    }

    #[test]
    fn kitty_uses_base_layout_key() {
        // Physical "A" key on a layout where it types "b".
        let k = key(b"\x1b[98::97;;98u");
        assert_eq!((k.key.as_str(), k.print.as_str()), ("A", "b"));
    }

    #[test]
    fn kitty_cmd_ctrl_c() {
        let k = key(b"\x1b[99;13u");
        assert!(k.cmd && k.ctrl && !k.shift && !k.opt);
        assert_eq!(k.key, "C");
        assert_eq!(k.print, "");
    }

    #[test]
    fn kitty_esc_and_space() {
        assert_eq!(key(b"\x1b[27u").key, "Esc");
        let space = key(b"\x1b[32;;32u");
        assert_eq!(space.key, "Space");
    }

    #[test]
    fn kitty_arrow_with_mods() {
        let k = key(b"\x1b[1;3A");
        assert!(k.opt);
        assert_eq!(k.key, "Up");
    }

    #[test]
    fn kitty_ignores_release_and_modifiers() {
        assert_eq!(parse_one(b"\x1b[97;1:3u"), (Parsed::Ignored, 9));
        assert_eq!(parse_one(b"\x1b[57441;2u"), (Parsed::Ignored, 10));
        assert_eq!(parse_one(b"\x1b[?31u"), (Parsed::Ignored, 6));
    }

    #[test]
    fn legacy_ctrl_c() {
        let k = key(b"\x03");
        assert!(k.ctrl);
        assert_eq!(k.key, "C");
    }

    #[test]
    fn legacy_chars_and_opt() {
        let k = key(b"B");
        assert!(k.shift);
        assert_eq!((k.key.as_str(), k.print.as_str()), ("B", "B"));
        let k = key(b"\x1bx");
        assert!(k.opt);
        assert_eq!(k.key, "X");
        assert_eq!(key("é".as_bytes()).key, "É");
    }

    #[test]
    fn incomplete_and_flush() {
        assert_eq!(parse_one(b"\x1b"), (Parsed::Incomplete, 0));
        assert_eq!(parse_one(b"\x1b[1;"), (Parsed::Incomplete, 0));
        assert_eq!(flush(b"\x1b").unwrap().key, "Esc");
    }
}
