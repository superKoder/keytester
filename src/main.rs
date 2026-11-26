mod parse;
mod table;

use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use parse::{KeyPress, Parsed};

const QUIT_PRESSES: u32 = 3;

/// Kitty keyboard protocol flags: disambiguate (1) + alternate keys (4) +
/// all keys as escape codes (8) + associated text (16).
const KITTY_FLAGS: u32 = 1 | 4 | 8 | 16;

fn main() {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        eprintln!("keytester: stdin and stdout must be a terminal");
        std::process::exit(1);
    }
    if let Err(e) = run() {
        eprintln!("keytester: {e}");
        std::process::exit(1);
    }
}

/// Restores the terminal on drop (normal exit, error, or panic).
struct TerminalGuard {
    kitty: bool,
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.kitty {
            let _ = write_flush("\x1b[<u");
        }
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

fn run() -> io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let mut guard = TerminalGuard { kitty: false };

    guard.kitty = detect_kitty()?;
    if guard.kitty {
        write_flush(&format!("\x1b[>{KITTY_FLAGS}u"))?;
        println_raw("Mode: kitty keyboard protocol")?;
    } else {
        println_raw("Mode: legacy (this terminal can't report Cmd or the unshifted key)")?;
    }
    println_raw("")?;
    println_raw(table::HEADER)?;
    println_raw(table::SEPARATOR)?;

    let mut quit = QuitCounter::default();
    let mut buf: Vec<u8> = Vec::new();
    loop {
        // A short timeout lets us resolve a lone ESC in legacy mode.
        let timeout = if buf.is_empty() { None } else { Some(Duration::from_millis(50)) };
        let mut keys = Vec::new();
        if read_into(&mut buf, timeout)? {
            loop {
                if buf.is_empty() {
                    break;
                }
                let (parsed, n) = parse::parse_one(&buf);
                match parsed {
                    Parsed::Incomplete => break,
                    Parsed::Key(k) => keys.push(k),
                    Parsed::Ignored => {}
                }
                buf.drain(..n);
            }
        } else {
            // Timed out with a partial sequence pending.
            keys.extend(parse::flush(&buf));
            buf.clear();
        }

        for k in keys {
            let note = quit.press(&k);
            println_raw(&table::row(&k, note))?;
            if quit.done() {
                return Ok(());
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuitKey {
    Esc,
    CtrlC,
}

#[derive(Default)]
struct QuitCounter {
    last: Option<QuitKey>,
    count: u32,
}

impl QuitCounter {
    /// Records a key press and returns the note to show next to it.
    fn press(&mut self, k: &KeyPress) -> &'static str {
        let qk = match k.key.as_str() {
            "Esc" => Some(QuitKey::Esc),
            "C" if k.ctrl => Some(QuitKey::CtrlC),
            _ => None,
        };
        match qk {
            None => {
                self.last = None;
                self.count = 0;
                ""
            }
            Some(q) => {
                self.count = if self.last == Some(q) { self.count + 1 } else { 1 };
                self.last = Some(q);
                if self.done() { "(QUITTING!)" } else { "(press 3 times to quit)" }
            }
        }
    }

    fn done(&self) -> bool {
        self.count >= QUIT_PRESSES
    }
}

/// Asks the terminal for its kitty keyboard flags, followed by a primary device
/// attributes request that every terminal answers. A kitty reply arriving before
/// the DA reply means the protocol is supported.
fn detect_kitty() -> io::Result<bool> {
    write_flush("\x1b[?u\x1b[c")?;
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut buf = Vec::new();
    let mut kitty = false;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if !read_into(&mut buf, Some(left))? {
            break;
        }
        // Scan complete CSI ? ... sequences.
        let mut i = 0;
        while let Some(start) = find(&buf[i..], b"\x1b[?").map(|p| p + i) {
            let Some(end) = buf[start + 3..].iter().position(|b| (0x40..=0x7e).contains(b)) else {
                break;
            };
            match buf[start + 3 + end] {
                b'u' => kitty = true,
                b'c' => return Ok(kitty),
                _ => {}
            }
            i = start + 3 + end + 1;
        }
    }
    Ok(kitty)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Waits up to `timeout` (forever if `None`) for input and appends it to `buf`.
/// Returns `false` on timeout.
fn read_into(buf: &mut Vec<u8>, timeout: Option<Duration>) -> io::Result<bool> {
    let mut pfd = libc::pollfd { fd: libc::STDIN_FILENO, events: libc::POLLIN, revents: 0 };
    let ms = timeout.map_or(-1, |t| t.as_millis().min(i32::MAX as u128) as i32);
    loop {
        // SAFETY: pfd is a valid pollfd and we pass a count of 1.
        let r = unsafe { libc::poll(&mut pfd, 1, ms) };
        if r < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        if r == 0 {
            return Ok(false);
        }
        break;
    }
    let mut chunk = [0u8; 1024];
    // SAFETY: chunk is a valid writable buffer of the given length.
    let n = unsafe { libc::read(libc::STDIN_FILENO, chunk.as_mut_ptr().cast(), chunk.len()) };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    if n == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stdin closed"));
    }
    buf.extend_from_slice(&chunk[..n as usize]);
    Ok(true)
}

fn write_flush(s: &str) -> io::Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(s.as_bytes())?;
    out.flush()
}

/// Raw mode disables newline translation, so emit `\r\n` explicitly.
fn println_raw(s: &str) -> io::Result<()> {
    write_flush(&format!("{s}\r\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kp(cmd: bool, ctrl: bool, key: &str) -> KeyPress {
        KeyPress { cmd, ctrl, key: key.into(), ..Default::default() }
    }

    #[test]
    fn readme_quit_sequence() {
        let mut q = QuitCounter::default();
        assert_eq!(q.press(&kp(false, false, "Esc")), "(press 3 times to quit)");
        assert_eq!(q.press(&kp(true, false, "X")), "");
        assert_eq!(q.press(&kp(true, true, "C")), "(press 3 times to quit)");
        assert_eq!(q.press(&kp(false, true, "C")), "(press 3 times to quit)");
        assert!(!q.done());
        assert_eq!(q.press(&kp(false, true, "C")), "(QUITTING!)");
        assert!(q.done());
    }

    #[test]
    fn switching_quit_keys_resets() {
        let mut q = QuitCounter::default();
        q.press(&kp(false, false, "Esc"));
        q.press(&kp(false, false, "Esc"));
        q.press(&kp(false, true, "C"));
        assert!(!q.done());
        q.press(&kp(false, false, "C")); // plain C is not a quit key
        assert_eq!(q.count, 0);
    }
}
