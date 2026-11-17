//! Attacks on the workspace folder: every case tries to make a path lead out of it, or to make a
//! call hang or leak. The outside of the workspace is a folder next to it with a secret file; after
//! each case it must be exactly as it was.

#![cfg(unix)]
// Test code: a failed `unwrap` is a failed test.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use construct_agent::workspace::{DIR_NAME, Error, Workspace};

const SECRET: &str = "OUTSIDE-SECRET";
const INSIDE: &str = "inside SECRET";
const ROUNDS: usize = 10_000;

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A data directory with the workspace in it and, next to it, a folder that must stay untouched.
struct Stand {
    base: PathBuf,
    ws: Workspace,
    initial: Vec<(String, Option<Vec<u8>>)>,
}

impl Stand {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!(
            "cb-ws-attack-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(base.join("outside/dir")).unwrap();
        fs::write(base.join("outside/secret.txt"), SECRET).unwrap();
        fs::write(base.join("outside/dir/inner.txt"), SECRET).unwrap();
        fs::write(base.join("outside/dir/f.txt"), SECRET).unwrap();
        let ws = Workspace::open(&base.join("data")).unwrap();
        let initial = snapshot(&base.join("outside"));
        Self { base, ws, initial }
    }

    fn root(&self) -> PathBuf {
        self.base.join("data").join(DIR_NAME)
    }

    fn outside(&self) -> PathBuf {
        self.base.join("outside")
    }

    /// A link `name` in the workspace that points to `target`.
    fn link(&self, target: impl AsRef<Path>, name: &str) {
        let path = self.root().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        symlink(target, path).unwrap();
    }

    /// The same folders and files, with the same content, as when the stand was made.
    fn assert_outside_untouched(&self) {
        assert_eq!(snapshot(&self.outside()), self.initial);
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

/// Removes a folder when dropped, also after a failed assertion.
struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn snapshot(dir: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    fn walk(root: &Path, dir: &Path, found: &mut Vec<(String, Option<Vec<u8>>)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let kind = fs::symlink_metadata(&path).unwrap().file_type();
            if kind.is_dir() {
                found.push((name, None));
                walk(root, &path, found);
            } else {
                found.push((name, fs::read(&path).ok()));
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, dir, &mut found);
    found.sort();
    found
}

/// Runs a call on a thread of its own and fails when it does not return in time.
fn returns_in_time<T: Send + 'static>(call: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(call());
    });
    receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("the call did not return")
}

fn never_secret(result: Result<Vec<String>, Error>) {
    if let Ok(lines) = result {
        // Anything of the outside files, also an edit of them ("OUTSIDE-LEAKED"), is a leak.
        assert!(!lines.iter().any(|l| l.contains("OUTSIDE")), "{lines:?}");
    }
}

// Paths

#[test]
fn climbing_paths_are_refused_by_every_call() {
    let s = Stand::new();
    s.ws.write("a/b/c.txt", "in").unwrap();
    for path in [
        "a/../../x",
        "a/../../outside/secret.txt",
        "a/b/../../../outside/secret.txt",
        "./../outside/secret.txt",
        "a//../..//outside/secret.txt",
        "../outside/secret.txt/",
        "a/b/../../..",
        "a/b/c.txt/../../../../x",
    ] {
        assert_eq!(
            s.ws.read(path, None, None),
            Err(Error::InvalidPath),
            "{path}"
        );
        assert_eq!(s.ws.write(path, "x"), Err(Error::InvalidPath), "{path}");
        assert_eq!(
            s.ws.edit(path, "S", "x", true),
            Err(Error::InvalidPath),
            "{path}"
        );
        assert_eq!(s.ws.glob("*", path), Err(Error::InvalidPath), "{path}");
    }
    s.assert_outside_untouched();
    assert!(!s.base.join("data/x").exists() && !s.base.join("x").exists());
}

#[test]
fn very_long_names_and_paths_fail_without_leaving() {
    let s = Stand::new();
    let longest = "n".repeat(255);
    s.ws.write(&longest, "ok").unwrap();
    assert_eq!(s.ws.read(&longest, None, None).unwrap(), ["ok"]);

    let too_long = "n".repeat(256);
    for result in [
        s.ws.write(&too_long, "x").unwrap_err(),
        s.ws.read(&too_long, None, None).unwrap_err(),
        s.ws.write(&format!("{}/f.txt", "d".repeat(5000)), "x")
            .unwrap_err(),
        s.ws.read(&"a/".repeat(4000), None, None).unwrap_err(),
    ] {
        let text = format!("{result} {result:?}");
        assert!(!text.contains('/') && !text.contains("nnnn"), "{text}");
    }
    // A path of many short parts is handled one part at a time.
    let deep = format!("{}f.txt", "a/".repeat(300));
    s.ws.write(&deep, "x").unwrap();
    assert_eq!(s.ws.read(&deep, None, None).unwrap(), ["x"]);
    s.ws.edit(&deep, "x", "y", false).unwrap();
    assert_eq!(s.ws.glob("**/*", "").unwrap(), [deep, longest]);
    // A path longer than the system's limit (4096 bytes on Linux, 1024 on macOS) is refused
    // with an error and nothing is made.
    let beyond = format!("{}f.txt", "b/".repeat(2200));
    assert!(s.ws.write(&beyond, "x").is_err());
    assert!(s.ws.read(&beyond, None, None).is_err());
    assert!(s.ws.edit(&beyond, "x", "y", false).is_err());
    assert!(!s.root().join("b").exists());
    s.assert_outside_untouched();
}

#[test]
fn unicode_names_stay_inside_and_round_trip() {
    let s = Stand::new();
    let names = [
        "é.txt",
        "e\u{301}.txt",
        "日本語/файл.txt",
        "\u{202e}txt.exe",
        "．．/x.txt",
        "..\u{2044}x.txt",
        "a\\..\\..\\x.txt",
        "...",
        "emoji-🙂.txt",
    ];
    // APFS folds the two spellings of "é" into one file, so only one of them is made there.
    let names: Vec<_> = names
        .into_iter()
        .filter(|name| !(cfg!(target_os = "macos") && *name == "e\u{301}.txt"))
        .collect();
    for (i, name) in names.iter().enumerate() {
        let text = format!("content {i}");
        s.ws.write(name, &text).unwrap();
        assert_eq!(
            s.ws.read(name, None, None).unwrap(),
            std::slice::from_ref(&text),
            "{name}"
        );
        s.ws.edit(name, "content", "changed", false).unwrap();
    }
    assert_eq!(s.ws.glob("**/*", "").unwrap().len(), names.len());
    // Everything went below the workspace folder.
    assert_eq!(fs::read_dir(s.base.join("data")).unwrap().count(), 1);
    s.assert_outside_untouched();
}

#[cfg(target_os = "macos")]
#[test]
fn names_that_differ_only_by_case_are_the_same_file_and_links_are_still_refused() {
    let s = Stand::new();
    s.ws.write("Dir/File.txt", "one").unwrap();
    assert_eq!(s.ws.read("dir/file.txt", None, None).unwrap(), ["one"]);
    assert_eq!(s.ws.read("DIR/FILE.TXT", None, None).unwrap(), ["one"]);
    s.link(s.outside().join("dir"), "Link");
    for path in ["Link/inner.txt", "link/inner.txt", "LINK/INNER.TXT"] {
        never_secret(s.ws.read(path, None, None));
        assert!(s.ws.write(&path.replace("inner", "new"), "x").is_err());
    }
    s.assert_outside_untouched();
}

// Links

#[test]
fn a_file_link_to_the_outside_is_refused() {
    for relative in [false, true] {
        let s = Stand::new();
        let target = if relative {
            PathBuf::from("../../outside/secret.txt")
        } else {
            s.outside().join("secret.txt")
        };
        s.link(target, "ln");
        assert!(s.ws.read("ln", None, None).is_err());
        assert!(s.ws.edit("ln", "SECRET", "x", false).is_err());
        s.assert_outside_untouched();
        let _ = s.ws.write("ln", "attack");
        s.assert_outside_untouched();
        assert!(!s.ws.glob("**/*", "").unwrap().contains(&"ln".to_owned()));
    }
}

#[test]
fn a_folder_link_to_the_outside_is_refused_by_every_call() {
    for relative in [false, true] {
        let s = Stand::new();
        let target = if relative {
            PathBuf::from("../../outside/dir")
        } else {
            s.outside().join("dir")
        };
        s.link(target, "dl");
        assert!(s.ws.read("dl/inner.txt", None, None).is_err());
        assert!(s.ws.write("dl/new.txt", "attack").is_err());
        assert!(s.ws.write("dl/sub/new.txt", "attack").is_err());
        assert!(s.ws.edit("dl/inner.txt", "SECRET", "x", false).is_err());
        assert!(s.ws.glob("*", "dl").is_err());
        assert!(s.ws.glob("**/*", "dl").is_err());
        assert!(
            s.ws.glob("**/*", "").unwrap().is_empty(),
            "links are not followed"
        );
        s.assert_outside_untouched();
    }
}

#[test]
fn a_link_in_the_middle_of_a_path_is_refused() {
    let s = Stand::new();
    s.ws.write("a/b/c.txt", "in").unwrap();
    s.link("../../../outside", "a/b/up");
    s.link(s.outside(), "a/abs");
    for path in [
        "a/b/up/secret.txt",
        "a/abs/secret.txt",
        "a/b/up/dir/inner.txt",
    ] {
        assert!(s.ws.read(path, None, None).is_err(), "{path}");
        assert!(s.ws.write(path, "attack").is_err(), "{path}");
        assert!(s.ws.edit(path, "SECRET", "x", false).is_err(), "{path}");
    }
    s.assert_outside_untouched();
}

#[test]
fn a_link_that_points_nowhere_never_gets_its_target_created() {
    let s = Stand::new();
    s.link(s.outside().join("missing.txt"), "dangling");
    s.link("../../outside/missing2.txt", "dangling-relative");
    s.link(s.outside().join("newdir"), "dangling-dir");
    s.link("../../outside/newdir2", "dangling-dir-relative");
    for name in ["dangling", "dangling-relative"] {
        assert!(s.ws.read(name, None, None).is_err());
        let _ = s.ws.write(name, "attack");
        let _ = s.ws.edit(name, "a", "b", false);
    }
    for name in ["dangling-dir", "dangling-dir-relative"] {
        assert!(s.ws.write(&format!("{name}/f.txt"), "attack").is_err());
        assert!(s.ws.write(&format!("{name}/a/b/f.txt"), "attack").is_err());
        assert!(s.ws.read(&format!("{name}/f.txt"), None, None).is_err());
    }
    s.assert_outside_untouched();
    for name in ["missing.txt", "missing2.txt", "newdir", "newdir2"] {
        assert!(!s.outside().join(name).exists(), "{name}");
    }
}

#[test]
fn link_loops_end_with_an_error() {
    let s = Stand::new();
    s.link("b", "a");
    s.link("a", "b");
    s.link("self", "self");
    s.link("loop-dir", "loop-dir");
    s.link("../workspace", "up");
    returns_in_time(move || {
        for name in [
            "a",
            "b",
            "self",
            "loop-dir",
            "up",
            "loop-dir/x",
            "up/up/up/x",
        ] {
            assert!(s.ws.read(name, None, None).is_err(), "{name}");
            assert!(s.ws.edit(name, "a", "b", false).is_err(), "{name}");
            let _ = s.ws.write(name, "x");
            let _ = s.ws.glob("**/*", name);
        }
        let _ = s.ws.glob("**/*", "");
        s.assert_outside_untouched();
    });
}

#[test]
fn links_that_stay_inside_work() {
    let s = Stand::new();
    s.ws.write("real.txt", "inside text").unwrap();
    s.ws.write("real/f.txt", "in folder").unwrap();
    s.link("real.txt", "ln");
    s.link("real", "dl");
    s.link("../real.txt", "real/up");
    assert_eq!(s.ws.read("ln", None, None).unwrap(), ["inside text"]);
    assert_eq!(s.ws.read("dl/f.txt", None, None).unwrap(), ["in folder"]);
    assert_eq!(s.ws.read("real/up", None, None).unwrap(), ["inside text"]);
    s.ws.write("dl/new.txt", "via link").unwrap();
    assert_eq!(s.ws.read("real/new.txt", None, None).unwrap(), ["via link"]);
    s.ws.edit("dl/f.txt", "folder", "link", false).unwrap();
    assert_eq!(s.ws.read("real/f.txt", None, None).unwrap(), ["in link"]);
    s.ws.edit("ln", "inside", "changed", false).unwrap();
    assert_eq!(s.ws.read("ln", None, None).unwrap(), ["changed text"]);
    s.assert_outside_untouched();
}

#[test]
fn a_root_that_is_a_link_confines_to_the_folder_it_points_to() {
    let base = std::env::temp_dir().join(format!(
        "cb-ws-attack-root-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _cleanup = RemoveOnDrop(base.clone());
    fs::create_dir_all(base.join("data")).unwrap();
    fs::create_dir_all(base.join("elsewhere/real-root")).unwrap();
    fs::create_dir_all(base.join("elsewhere/outside")).unwrap();
    fs::write(base.join("elsewhere/secret.txt"), SECRET).unwrap();
    fs::write(base.join("elsewhere/outside/secret.txt"), SECRET).unwrap();
    symlink(
        base.join("elsewhere/real-root"),
        base.join("data").join(DIR_NAME),
    )
    .unwrap();

    let ws = Workspace::open(&base.join("data")).unwrap();
    ws.write("a.txt", "in").unwrap();
    assert_eq!(
        fs::read_to_string(base.join("elsewhere/real-root/a.txt")).unwrap(),
        "in"
    );
    symlink(
        base.join("elsewhere/outside"),
        base.join("elsewhere/real-root/out"),
    )
    .unwrap();
    for path in ["../secret.txt", "out/secret.txt", "../outside/secret.txt"] {
        never_secret(ws.read(path, None, None));
        assert!(ws.read(path, None, None).is_err(), "{path}");
        assert!(ws.write(path, "attack").is_err(), "{path}");
        assert!(ws.edit(path, "SECRET", "x", false).is_err(), "{path}");
    }
    assert_eq!(
        fs::read_to_string(base.join("elsewhere/secret.txt")).unwrap(),
        SECRET
    );
    assert_eq!(
        fs::read_to_string(base.join("elsewhere/outside/secret.txt")).unwrap(),
        SECRET
    );

    // A root link that points nowhere is not created and not followed.
    let dangling = base.join("data2");
    fs::create_dir_all(&dangling).unwrap();
    symlink(base.join("elsewhere/missing-root"), dangling.join(DIR_NAME)).unwrap();
    assert!(Workspace::open(&dangling).is_err());
    assert!(!base.join("elsewhere/missing-root").exists());
}

// Races

/// Three threads (read, write, edit) run `ROUNDS` rounds each on `path` while `swap` runs in a loop
/// on a fourth thread. `swap` returns whether the link to the outside was in place. The swapper is
/// stopped before a worker's panic is raised, so a leak fails the test and does not hang it.
fn race(s: &Stand, path: &str, swap: impl Fn() -> bool + Sync) {
    let stop = AtomicBool::new(false);
    let linked = AtomicUsize::new(0);
    // Calls the workspace refused because the path met a link (or a file that is not a regular one).
    let refused = AtomicUsize::new(0);
    let count = |result: Result<(), Error>| {
        if matches!(
            result,
            Err(Error::InvalidPath | Error::Io(_) | Error::NotRegularFile)
        ) {
            refused.fetch_add(1, Ordering::Relaxed);
        }
    };
    let outcomes = thread::scope(|scope| {
        scope.spawn(|| {
            while !stop.load(Ordering::Relaxed) {
                if swap() {
                    linked.fetch_add(1, Ordering::Relaxed);
                }
            }
        });
        let workers = [
            scope.spawn(|| {
                for _ in 0..ROUNDS {
                    let result = s.ws.read(path, None, None);
                    count(result.clone().map(|_| ()));
                    never_secret(result);
                }
            }),
            scope.spawn(|| {
                for _ in 0..ROUNDS {
                    count(s.ws.write(path, INSIDE));
                }
            }),
            // An edit that reached the outside file leaves "LEAKED" there: nothing in this loop
            // matches the outside text again, so the snapshot shows it.
            scope.spawn(|| {
                for _ in 0..ROUNDS {
                    count(s.ws.edit(path, "SECRET", "LEAKED", true));
                    count(s.ws.edit(path, "inside LEAKED", "inside SECRET", true));
                }
            }),
        ];
        let outcomes: Vec<_> = workers
            .into_iter()
            .map(thread::ScopedJoinHandle::join)
            .collect();
        stop.store(true, Ordering::Relaxed);
        outcomes
    });
    for outcome in outcomes {
        outcome.unwrap();
    }
    let (linked, refused) = (
        linked.load(Ordering::Relaxed),
        refused.load(Ordering::Relaxed),
    );
    assert!(linked > 0, "the link was never in place");
    eprintln!("{path}: link made {linked} times, {refused} calls refused");
    assert!(refused > 0, "no call met the link");
    s.assert_outside_untouched();
}

#[test]
fn a_folder_swapped_for_a_link_while_the_calls_run() {
    let s = Stand::new();
    let folder = s.root().join("d");
    let target = s.outside().join("dir");
    race(&s, "d/f.txt", || {
        let _ = fs::remove_dir_all(&folder);
        let linked = symlink(&target, &folder).is_ok();
        let _ = fs::remove_file(&folder);
        let _ = fs::create_dir(&folder);
        let _ = fs::write(folder.join("f.txt"), INSIDE);
        linked
    });
}

#[test]
fn a_file_swapped_for_a_link_while_the_calls_run() {
    let s = Stand::new();
    let file = s.root().join("f.txt");
    let temp = s.root().join(".swap");
    let target = s.outside().join("secret.txt");
    race(&s, "f.txt", || {
        let _ = fs::remove_file(&temp);
        let _ = symlink(&target, &temp);
        let linked = fs::rename(&temp, &file).is_ok();
        let _ = fs::write(&temp, INSIDE);
        let _ = fs::rename(&temp, &file);
        linked
    });
}

#[test]
fn a_folder_in_the_middle_swapped_for_a_link_while_the_calls_run() {
    let s = Stand::new();
    let folder = s.root().join("a/b");
    let target = s.outside().join("dir");
    race(&s, "a/b/f.txt", || {
        let _ = fs::remove_dir_all(&folder);
        let linked = symlink(&target, &folder).is_ok();
        let _ = fs::remove_file(&folder);
        let _ = fs::create_dir_all(&folder);
        let _ = fs::write(folder.join("f.txt"), INSIDE);
        linked
    });
}

// Special files

fn make_fifo(path: &Path) {
    let status = Command::new("mkfifo").arg(path).status().unwrap();
    assert!(status.success());
}

#[test]
fn a_named_pipe_does_not_hang_any_call() {
    let s = Stand::new();
    make_fifo(&s.root().join("pipe"));
    make_fifo(&s.root().join("d.pipe"));
    s.ws.write("ok.txt", "x").unwrap();
    returns_in_time(move || {
        assert_eq!(s.ws.read("pipe", None, None), Err(Error::NotRegularFile));
        assert_eq!(
            s.ws.edit("pipe", "a", "b", false),
            Err(Error::NotRegularFile)
        );
        assert_eq!(s.ws.write("pipe", "x"), Err(Error::NotRegularFile));
        assert!(s.ws.glob("*", "pipe").is_err());
        assert!(s.ws.read("pipe/x", None, None).is_err());
        assert_eq!(s.ws.glob("*", "").unwrap(), ["ok.txt"]);
    });
}

#[test]
fn a_socket_is_refused() {
    let s = Stand::new();
    let _listener = UnixListener::bind(s.root().join("sock")).unwrap();
    returns_in_time(move || {
        assert!(s.ws.read("sock", None, None).is_err());
        assert!(s.ws.edit("sock", "a", "b", false).is_err());
        assert_eq!(s.ws.write("sock", "x"), Err(Error::NotRegularFile));
        assert!(s.ws.glob("*", "").unwrap().is_empty());
    });
}

/// Covers the refusal of a link that points outside to a device. A device node inside the
/// workspace needs root to make, so the `NotRegularFile` path for devices is not reached here.
#[test]
fn a_link_to_a_device_is_refused_and_does_not_hang() {
    let s = Stand::new();
    for (i, device) in ["/dev/zero", "/dev/null", "/dev/urandom", "/dev/full"]
        .iter()
        .enumerate()
    {
        s.link(device, &format!("dev{i}"));
    }
    returns_in_time(move || {
        for i in 0..4 {
            let name = format!("dev{i}");
            assert!(s.ws.read(&name, None, None).is_err(), "{name}");
            assert!(s.ws.edit(&name, "a", "b", false).is_err(), "{name}");
            let _ = s.ws.write(&name, "x");
        }
    });
}

#[test]
fn a_hard_link_to_a_file_outside_is_never_written_through() {
    let s = Stand::new();
    fs::hard_link(s.outside().join("secret.txt"), s.root().join("hard.txt")).unwrap();
    s.ws.edit("hard.txt", "SECRET", "attack", false).unwrap();
    s.assert_outside_untouched();
    assert_eq!(
        s.ws.read("hard.txt", None, None).unwrap(),
        ["OUTSIDE-attack"]
    );

    fs::hard_link(s.outside().join("dir/f.txt"), s.root().join("hard2.txt")).unwrap();
    s.ws.write("hard2.txt", "attack").unwrap();
    s.assert_outside_untouched();
}

#[test]
fn a_file_that_grows_while_it_is_read_gives_whole_text() {
    let s = Stand::new();
    s.ws.write("grow.txt", "line 0\n").unwrap();
    let path = s.root().join("grow.txt");
    let done = AtomicBool::new(false);
    let mut sizes = std::collections::BTreeSet::new();
    thread::scope(|scope| {
        scope.spawn(|| {
            let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
            for i in 1..=3000 {
                file.write_all(format!("line {i}\n").as_bytes()).unwrap();
                if i % 50 == 0 {
                    thread::sleep(Duration::from_micros(200));
                }
            }
            done.store(true, Ordering::Relaxed);
        });
        // Read until the writer is done, so the reads overlap the appends.
        while !done.load(Ordering::Relaxed) {
            let lines = s.ws.read("grow.txt", None, None).unwrap();
            let last = lines.len() - 1;
            for (i, line) in lines.iter().enumerate() {
                let whole = format!("line {i}");
                // The read can come between the two copies of an append that crosses a page, so
                // the last line may be cut short; every other line is whole.
                assert!(
                    line == &whole || (i == last && whole.starts_with(line.as_str())),
                    "{line}"
                );
            }
            sizes.insert(lines.len());
        }
    });
    assert!(sizes.len() > 1, "no read saw the file grow: {sizes:?}");
    assert_eq!(s.ws.read("grow.txt", None, None).unwrap().len(), 3001);
}

// The root changes under the agent

#[test]
fn a_renamed_root_keeps_working_on_the_same_folder() {
    let s = Stand::new();
    s.ws.write("a.txt", "one").unwrap();
    let moved = s.base.join("data/moved");
    fs::rename(s.root(), &moved).unwrap();
    // Another folder takes the old name, with a file the agent must not see.
    fs::create_dir(s.root()).unwrap();
    fs::write(s.root().join("imposter.txt"), SECRET).unwrap();

    assert_eq!(s.ws.read("a.txt", None, None).unwrap(), ["one"]);
    assert_eq!(s.ws.read("imposter.txt", None, None), Err(Error::NotFound));
    s.ws.write("b.txt", "two").unwrap();
    s.ws.write("sub/c.txt", "three").unwrap();
    assert_eq!(fs::read_to_string(moved.join("b.txt")).unwrap(), "two");
    assert_eq!(
        fs::read_to_string(moved.join("sub/c.txt")).unwrap(),
        "three"
    );
    assert!(!s.root().join("b.txt").exists());
    assert_eq!(
        s.ws.glob("**/*", "").unwrap(),
        ["a.txt", "b.txt", "sub/c.txt"]
    );
    assert_eq!(
        fs::read_to_string(s.root().join("imposter.txt")).unwrap(),
        SECRET
    );
}

#[test]
fn a_deleted_root_fails_closed_and_is_not_recreated() {
    let s = Stand::new();
    s.ws.write("a.txt", "one").unwrap();
    fs::remove_dir_all(s.root()).unwrap();

    assert!(s.ws.read("a.txt", None, None).is_err());
    assert!(s.ws.write("b.txt", "x").is_err());
    assert!(s.ws.write("sub/c.txt", "x").is_err());
    assert!(s.ws.edit("a.txt", "one", "two", false).is_err());
    assert!(s.ws.glob("**/*", "").map_or(true, |found| found.is_empty()));
    assert!(!s.root().exists(), "the folder is not made again");
    s.assert_outside_untouched();
}

// Permissions

/// Runs the test again in a child process with umask 0, so that a mode the code leaves to the umask
/// shows as open to others whatever umask the test runs under.
#[test]
fn files_and_folders_made_are_for_the_user_only() {
    const CHILD: &str = "CB_WS_ATTACK_UMASK_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let status = Command::new("sh")
            .args(["-c", "umask 0; exec \"$0\" --exact \"$1\" --test-threads=1"])
            .arg(std::env::current_exe().unwrap())
            .arg("files_and_folders_made_are_for_the_user_only")
            .env(CHILD, "1")
            .status()
            .unwrap();
        assert!(status.success(), "failed under umask 0");
        return;
    }
    let s = Stand::new();
    s.ws.write("top.txt", "x").unwrap();
    s.ws.write("a/b/c.txt", "x").unwrap();
    let mode = |p: &str| fs::metadata(s.root().join(p)).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(""), 0o700);
    assert_eq!(mode("top.txt") & 0o077, 0, "{:o}", mode("top.txt"));
    assert_eq!(mode("a"), 0o700);
    assert_eq!(mode("a/b"), 0o700);
    assert_eq!(mode("a/b/c.txt") & 0o077, 0, "{:o}", mode("a/b/c.txt"));

    // A file that was open to others is replaced by one that is not.
    fs::write(s.root().join("open.txt"), "x").unwrap();
    fs::set_permissions(s.root().join("open.txt"), fs::Permissions::from_mode(0o666)).unwrap();
    s.ws.write("open.txt", "y").unwrap();
    s.ws.edit("top.txt", "x", "y", false).unwrap();
    assert_eq!(mode("open.txt") & 0o077, 0);
    assert_eq!(mode("top.txt") & 0o077, 0);
}

// Errors and logs

#[test]
fn no_error_text_or_log_line_holds_a_path_or_file_content() {
    let s = Stand::new();
    s.ws.write("d/f.txt", "FILE-CONTENT-ONE FILE-CONTENT-ONE")
        .unwrap();
    s.ws.write("bin.txt", "FILE-CONTENT-TWO").unwrap();
    fs::write(s.root().join("bad.txt"), b"FILE-CONTENT-THREE\xff").unwrap();
    s.link(s.outside().join("secret.txt"), "ln");
    s.link(s.outside().join("dir"), "dl");
    s.link(s.outside().join("missing"), "dangling");
    s.link("self", "self");
    make_fifo(&s.root().join("pipe"));

    let errors = [
        s.ws.read("../outside/secret.txt", None, None),
        s.ws.read("ln", None, None),
        s.ws.read("dl/inner.txt", None, None),
        s.ws.read("dangling", None, None),
        s.ws.read("self", None, None),
        s.ws.read("pipe", None, None),
        s.ws.read("d", None, None),
        s.ws.read("bad.txt", None, None),
        s.ws.read(&"n".repeat(300), None, None),
    ]
    .into_iter()
    .map(Result::unwrap_err)
    .chain([
        s.ws.write("dl/new.txt", "x").unwrap_err(),
        s.ws.write("d", "x").unwrap_err(),
        s.ws.edit("d/f.txt", "FILE-CONTENT-ONE", "x", false)
            .unwrap_err(),
        s.ws.edit("d/f.txt", "FILE-CONTENT-NONE", "x", false)
            .unwrap_err(),
        s.ws.edit("bad.txt", "FILE-CONTENT-THREE", "x", false)
            .unwrap_err(),
        s.ws.glob("*", "dl").unwrap_err(),
        s.ws.glob("*", "d/f.txt").unwrap_err(),
        Workspace::open(&s.root().join("bad.txt")).unwrap_err(),
    ])
    .collect::<Vec<_>>();
    let base = s.base.to_string_lossy().into_owned();
    for error in errors {
        let text = format!("{error} {error:?}");
        for forbidden in [
            base.as_str(),
            "/tmp",
            "/dev",
            "outside",
            "secret.txt",
            "FILE-CONTENT",
            "OUTSIDE-SECRET",
            "d/f.txt",
            "dangling",
        ] {
            assert!(!text.contains(forbidden), "{forbidden}: {text}");
        }
    }
}

#[test]
fn the_module_writes_no_log_line() {
    let source = include_str!("../src/workspace.rs");
    let code = source.split("#[cfg(test)]").next().unwrap();
    for call in [
        "println!",
        "eprintln!",
        "print!",
        "dbg!",
        "tracing::",
        "log::",
    ] {
        assert!(!code.contains(call), "{call}");
    }
}
