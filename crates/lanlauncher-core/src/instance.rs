//! One launcher per user on Linux, and whether it may hide in the tray.
//!
//! A launcher whose window was closed keeps running in the tray, so the next
//! start from the menu must bring that window back instead of starting a
//! second launcher next to the sync engine. Windows and macOS get this from
//! `tauri-plugin-single-instance`; on Linux that plugin needs a D-Bus session
//! bus (it unwraps one) and would hand the graphics ladder's successor over
//! to the predecessor it is about to replace. Hence this: an `flock`ed file
//! says who is first, a Unix socket next to it carries "show your window".
//!
//! The lock goes with the process, crash or not, and the files are opened
//! close-on-exec, so neither the sync engine nor a game inherits it.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const LOCK: &str = "instance.lock";
const SOCKET: &str = "instance.sock";
const SHOW: &str = "show";
const QUIT: &str = "quit";

/// How long a second start waits for the first one to answer. The first one
/// answers from its own thread, so this only runs out on a launcher that is
/// stuck or on its way out.
const ANSWER_WITHIN: Duration = Duration::from_secs(2);

/// How the claim ended.
#[derive(Debug)]
pub enum Start {
    /// This process is the launcher; keep the value alive (see
    /// [`Instance::serve`]).
    Primary(Instance),
    /// Another launcher showed its window; this process has nothing to do.
    Handed,
    /// Another launcher holds the lock but neither answered nor went away
    /// in time, or the directory is not safe to use. Running anyway is what
    /// every launcher before this one did.
    Unguarded(String),
}

/// What a start asks of a launcher that is already running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// Show your window; this start then has nothing to do.
    Show,
    /// Quit, this start takes over: it carries arguments (`--safe-graphics`,
    /// `--demo`) the running one never read.
    Replace,
    /// Nothing while the given process lives — the graphics ladder's
    /// successor: its predecessor still holds the lock for a second or two
    /// and would answer "shown" for a window that is about to go. It waits
    /// for the lock, and only once that process is gone asks whoever beat it
    /// to the lock. (Not the parent: inside an AppImage that is the AppImage
    /// runtime, which lives as long as the launcher.)
    AfterExitOf(u32),
}

/// The lock and the socket of the launcher that came first.
#[derive(Debug)]
pub struct Instance {
    _lock: File,
    listener: UnixListener,
}

/// `$XDG_RUNTIME_DIR/<app id>`, which belongs to the user and is cleared at
/// logout; without it a per-user folder in the temp directory.
pub fn default_dir(app_id: &str) -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        Some(runtime) => PathBuf::from(runtime).join(app_id),
        None => {
            // SAFETY: getuid cannot fail.
            let uid = unsafe { libc::getuid() };
            std::env::temp_dir().join(format!("{app_id}-{uid}"))
        }
    }
}

/// Become the launcher, or have the running one show itself or quit.
///
/// `wait` is how long to wait for the lock when the running launcher did
/// not take the start over.
pub fn claim(dir: &Path, ask: Ask, wait: Duration) -> std::io::Result<Start> {
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    // A folder in a shared temp directory someone else made first — or a
    // symlink they placed there, pointing anywhere — would let them answer
    // for us. Not followed: once it is a real folder of ours, the sticky
    // temp directory keeps others from swapping it.
    let meta = std::fs::symlink_metadata(dir)?;
    // SAFETY: getuid cannot fail.
    if !meta.is_dir() || meta.uid() != unsafe { libc::getuid() } {
        return Ok(Start::Unguarded(format!(
            "{} is not a folder of this user",
            dir.display()
        )));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let lock = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join(LOCK))?;
    if try_lock(&lock)? {
        return primary(dir, lock).map(Start::Primary);
    }
    let socket = dir.join(SOCKET);
    match ask {
        Ask::Show => {
            if request(&socket, SHOW, ANSWER_WITHIN) {
                return Ok(Start::Handed);
            }
        }
        // Whatever it answers, the lock says when it is gone.
        Ask::Replace => {
            request(&socket, QUIT, ANSWER_WITHIN);
        }
        Ask::AfterExitOf(_) => {}
    }
    let until = Instant::now() + wait;
    let mut asked = Instant::now();
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
        if try_lock(&lock)? {
            return primary(dir, lock).map(Start::Primary);
        }
        // Another start may have got the lock first (a double click, or
        // the ladder's successor and a start from the menu). A plain start
        // then has nothing to do once that one shows its window; the
        // launcher that is leaving says "busy" until it is gone. A start
        // with options keeps asking whoever holds the lock to make way, or
        // its options would be lost. The ladder's successor asks only once
        // its predecessor is gone, which would answer "shown" for a window
        // that is about to go.
        let what = match ask {
            Ask::Show => Some(SHOW),
            Ask::Replace => Some(QUIT),
            Ask::AfterExitOf(pid) => (!alive(pid)).then_some(SHOW),
        };
        if let Some(what) = what {
            if asked.elapsed() >= Duration::from_secs(1) {
                asked = Instant::now();
                if request(&socket, what, Duration::ZERO) && what == SHOW {
                    return Ok(Start::Handed);
                }
            }
        }
    }
    Ok(Start::Unguarded(format!(
        "another launcher holds {} and did not answer within {} s",
        dir.join(LOCK).display(),
        (ANSWER_WITHIN + wait).as_secs()
    )))
}

/// A process that has ended but was not reaped yet counts as gone.
fn alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 only checks whether the process exists.
    let exists = unsafe { libc::kill(pid, 0) } == 0
        || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
    if !exists {
        return false;
    }
    // `pid (comm) S …`: the state follows the last parenthesis.
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => {
            stat.rsplit_once(')')
                .and_then(|(_, rest)| rest.trim_start().chars().next())
                != Some('Z')
        }
        Err(_) => true,
    }
}

fn try_lock(file: &File) -> std::io::Result<bool> {
    // SAFETY: a valid descriptor owned by `file`.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        return Ok(true);
    }
    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(false)
    } else {
        Err(err)
    }
}

fn primary(dir: &Path, lock: File) -> std::io::Result<Instance> {
    let path = dir.join(SOCKET);
    // Whoever bound it last is gone, or the lock would not be ours.
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(&path)?;
    Ok(Instance {
        _lock: lock,
        listener,
    })
}

/// True once the running launcher said it did what was asked. `connect_for`
/// covers the instant between the first launcher taking the lock and binding
/// its socket.
fn request(path: &Path, what: &str, connect_for: Duration) -> bool {
    let until = Instant::now() + connect_for;
    let stream = loop {
        match UnixStream::connect(path) {
            Ok(s) => break s,
            Err(_) if Instant::now() < until => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => return false,
        }
    };
    let _ = stream.set_read_timeout(Some(ANSWER_WITHIN));
    let _ = stream.set_write_timeout(Some(ANSWER_WITHIN));
    if (&stream).write_all(format!("{what}\n").as_bytes()).is_err() {
        return false;
    }
    read_line(&stream).trim() == "ok"
}

impl Instance {
    /// Answer later starts on a thread of its own for the rest of the
    /// process. `show` brings the window up and says whether there was one
    /// to bring — a launcher that is shutting down says no, and the new start
    /// then waits for it to finish and takes over. `quit` ends this launcher
    /// for a start that replaces it; that start waits for the lock.
    pub fn serve(self, show: impl Fn() -> bool + Send + 'static, quit: impl Fn() + Send + 'static) {
        std::thread::Builder::new()
            .name("single-instance".into())
            .spawn(move || {
                // Holds the lock for as long as the thread runs, which is as
                // long as the process does.
                let Instance { _lock, listener } = self;
                for stream in listener.incoming().flatten() {
                    let _ = stream.set_read_timeout(Some(ANSWER_WITHIN));
                    let _ = stream.set_write_timeout(Some(ANSWER_WITHIN));
                    match read_line(&stream).trim() {
                        SHOW => {
                            let answer = if show() { "ok\n" } else { "busy\n" };
                            let _ = (&stream).write_all(answer.as_bytes());
                        }
                        QUIT => {
                            let _ = (&stream).write_all(b"ok\n");
                            quit();
                        }
                        _ => {}
                    }
                }
            })
            .expect("spawn the single-instance thread");
    }
}

/// One short line, never more than a few bytes: whatever else arrives on the
/// socket is not ours to buffer.
fn read_line(stream: &UnixStream) -> String {
    let mut line = String::new();
    let _ = BufReader::new(stream.take(64)).read_line(&mut line);
    line
}

/// Steam's Game Mode on the Deck runs every program inside gamescope. There
/// is no tray to come back from, and Steam counts a hidden launcher as a
/// game that is still running.
pub fn in_gamescope(var: impl Fn(&str) -> Option<String>) -> bool {
    var("GAMESCOPE_WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
        || var("XDG_CURRENT_DESKTOP")
            .is_some_and(|v| v.split(':').any(|d| d.eq_ignore_ascii_case("gamescope")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn a_second_start_has_the_first_one_show_its_window() {
        let dir = tempfile::tempdir().unwrap();
        let Start::Primary(first) = claim(dir.path(), Ask::Show, Duration::ZERO).unwrap() else {
            panic!("the first start must be the launcher");
        };
        let shown = Arc::new(AtomicUsize::new(0));
        let counter = shown.clone();
        first.serve(
            move || {
                counter.fetch_add(1, Ordering::SeqCst);
                true
            },
            || {},
        );
        let second = claim(dir.path(), Ask::Show, Duration::ZERO).unwrap();
        assert!(matches!(second, Start::Handed), "{second:?}");
        assert_eq!(shown.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_launcher_on_its_way_out_is_waited_for_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let Start::Primary(first) = claim(dir.path(), Ask::Show, Duration::ZERO).unwrap() else {
            panic!("the first start must be the launcher");
        };
        // Answers "busy" and lets go of the lock shortly after, the way a
        // launcher stopping its sync engine does.
        let Instance { _lock, listener } = first;
        let leaving = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            read_line(&stream);
            (&stream).write_all(b"busy\n").unwrap();
            std::thread::sleep(Duration::from_millis(300));
            drop(_lock);
        });
        let second = claim(dir.path(), Ask::Show, Duration::from_secs(5)).unwrap();
        assert!(matches!(second, Start::Primary(_)), "{second:?}");
        leaving.join().unwrap();
    }

    #[test]
    fn the_ladders_successor_never_hands_over_to_its_predecessor() {
        let dir = tempfile::tempdir().unwrap();
        let Start::Primary(first) = claim(dir.path(), Ask::Show, Duration::ZERO).unwrap() else {
            panic!("the first start must be the launcher");
        };
        let shown = Arc::new(AtomicUsize::new(0));
        let counter = shown.clone();
        // Only the lock leaves; the socket keeps answering "shown".
        let Instance { _lock, listener } = first;
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                counter.fetch_add(1, Ordering::SeqCst);
                let _ = (&stream).write_all(b"ok\n");
            }
        });
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(_lock);
        });
        let successor = claim(
            dir.path(),
            Ask::AfterExitOf(std::process::id()),
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(matches!(successor, Start::Primary(_)), "{successor:?}");
        assert_eq!(shown.load(Ordering::SeqCst), 0);
        release.join().unwrap();
    }

    #[test]
    fn a_start_with_new_arguments_replaces_the_running_launcher() {
        let dir = tempfile::tempdir().unwrap();
        let Start::Primary(first) = claim(dir.path(), Ask::Show, Duration::ZERO).unwrap() else {
            panic!("the first start must be the launcher");
        };
        // The lock leaves with the launcher, the way `app.exit` ends it.
        let Instance { _lock, listener } = first;
        let lock = std::sync::Mutex::new(Some(_lock));
        let first = Instance {
            _lock: File::open(dir.path().join(LOCK)).unwrap(),
            listener,
        };
        first.serve(
            || panic!("a replacing start must not ask for the window"),
            move || drop(lock.lock().unwrap().take()),
        );
        let second = claim(dir.path(), Ask::Replace, Duration::from_secs(5)).unwrap();
        assert!(matches!(second, Start::Primary(_)), "{second:?}");
    }

    #[test]
    fn a_start_waiting_for_a_leaving_launcher_hands_over_to_whoever_came_next() {
        let dir = tempfile::tempdir().unwrap();
        let Start::Primary(leaving) = claim(dir.path(), Ask::Show, Duration::ZERO).unwrap() else {
            panic!("the first start must be the launcher");
        };
        // Says "busy" to everyone, like a launcher stopping its engine.
        leaving.serve(|| false, || {});
        // Someone else holds the lock now and shows its window when asked
        // (stands in for a start that won the race for the lock).
        let path = dir.path().to_path_buf();
        let shown = Arc::new(AtomicUsize::new(0));
        let counter = shown.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let _ = std::fs::remove_file(path.join(SOCKET));
            let listener = UnixListener::bind(path.join(SOCKET)).unwrap();
            for stream in listener.incoming().flatten() {
                if read_line(&stream).trim() == SHOW {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let _ = (&stream).write_all(b"ok\n");
                }
            }
        });
        let second = claim(dir.path(), Ask::Show, Duration::from_secs(5)).unwrap();
        assert!(matches!(second, Start::Handed), "{second:?}");
        assert_eq!(shown.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_start_with_options_keeps_asking_until_it_is_the_launcher() {
        let dir = tempfile::tempdir().unwrap();
        let Start::Primary(holder) = claim(dir.path(), Ask::Show, Duration::ZERO).unwrap() else {
            panic!("the first start must be the launcher");
        };
        // The first "quit" is lost (a launcher that won the race for the
        // lock after the one asked had gone); only a later one is obeyed.
        let Instance { _lock, listener } = holder;
        let quits = Arc::new(AtomicUsize::new(0));
        let counter = quits.clone();
        std::thread::spawn(move || {
            let mut lock = Some(_lock);
            for stream in listener.incoming().flatten() {
                if read_line(&stream).trim() == QUIT {
                    let _ = (&stream).write_all(b"ok\n");
                    if counter.fetch_add(1, Ordering::SeqCst) >= 1 {
                        drop(lock.take());
                    }
                }
            }
        });
        let start = claim(dir.path(), Ask::Replace, Duration::from_secs(5)).unwrap();
        assert!(matches!(start, Start::Primary(_)), "{start:?}");
        assert!(quits.load(Ordering::SeqCst) >= 2);
    }

    #[test]
    fn the_ladders_successor_asks_whoever_holds_the_lock_once_its_predecessor_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let Start::Primary(holder) = claim(dir.path(), Ask::Show, Duration::ZERO).unwrap() else {
            panic!("the first start must be the launcher");
        };
        let shown = Arc::new(AtomicUsize::new(0));
        let counter = shown.clone();
        holder.serve(
            move || {
                counter.fetch_add(1, Ordering::SeqCst);
                true
            },
            || {},
        );
        let mut gone = std::process::Command::new("true").spawn().unwrap();
        let pid = gone.id();
        gone.wait().unwrap();
        let successor = claim(dir.path(), Ask::AfterExitOf(pid), Duration::from_secs(5)).unwrap();
        assert!(matches!(successor, Start::Handed), "{successor:?}");
        assert_eq!(shown.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_symlink_in_place_of_the_folder_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        let link = dir.path().join("instance");
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
        let start = claim(&link, Ask::Show, Duration::ZERO).unwrap();
        assert!(matches!(start, Start::Unguarded(_)), "{start:?}");
        assert!(!elsewhere.join(LOCK).exists());
    }

    #[test]
    fn a_socket_left_by_a_crash_does_not_block_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(SOCKET), b"").unwrap();
        let start = claim(dir.path(), Ask::Show, Duration::ZERO).unwrap();
        assert!(matches!(start, Start::Primary(_)), "{start:?}");
    }

    #[test]
    fn a_launcher_that_never_answers_does_not_keep_the_next_one_from_starting() {
        let dir = tempfile::tempdir().unwrap();
        // Holds lock and socket, never accepts.
        let _first = claim(dir.path(), Ask::Show, Duration::ZERO).unwrap();
        let second = claim(dir.path(), Ask::Show, Duration::from_millis(200)).unwrap();
        assert!(matches!(second, Start::Unguarded(_)), "{second:?}");
    }

    #[test]
    fn game_mode_is_recognised() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert!(in_gamescope(env(&[(
            "GAMESCOPE_WAYLAND_DISPLAY",
            "gamescope-0"
        )])));
        assert!(in_gamescope(env(&[("XDG_CURRENT_DESKTOP", "gamescope")])));
        assert!(!in_gamescope(env(&[("XDG_CURRENT_DESKTOP", "KDE")])));
        assert!(!in_gamescope(env(&[])));
    }
}
