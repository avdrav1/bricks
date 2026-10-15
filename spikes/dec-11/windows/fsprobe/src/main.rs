//! DEC-11 spike (throwaway): how the file operations the suite relies on behave on this OS.
//! Each line: probe name, then Ok or the error. Mirrors suite_file::save_atomic (temp,
//! fsync, rename, fsync the directory), Brick's memory-mapped sources and scratch copies,
//! suite_file::recovery's owner locks, and the folders the apps derive from the environment.
use memmap2::Mmap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

fn show(name: &str, r: std::io::Result<impl std::fmt::Debug>) {
    match r {
        Ok(v) => println!("{name}: Ok {v:?}"),
        Err(e) => println!("{name}: Err {e} (raw {:?})", e.raw_os_error()),
    }
}

fn fresh(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
    let p = dir.join(name);
    let _ = fs::remove_file(&p);
    fs::write(&p, body).unwrap();
    p
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("lock-child") {
        let f = File::open(&args[2]).unwrap();
        match f.try_lock() {
            Ok(()) => println!("acquired"),
            Err(fs::TryLockError::WouldBlock) => println!("would-block"),
            Err(fs::TryLockError::Error(e)) => println!("error {e}"),
        }
        return;
    }
    let dir = std::env::temp_dir().join(format!("fsprobe-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    println!("os={} dir={}", std::env::consts::OS, dir.display());

    // save_atomic: temp file, fsync, rename over the destination, fsync the directory.
    let dest = fresh(&dir, "plain.csv", b"old");
    let tmp = fresh(&dir, "plain.tmp", b"new");
    show("rename over closed file", fs::rename(&tmp, &dest));
    show("open dir and sync_all (save_atomic's last step)", File::open(&dir).and_then(|d| d.sync_all()));

    // The destination open elsewhere with std's default sharing (read, write, delete).
    let dest = fresh(&dir, "open.csv", b"old");
    let held = File::open(&dest).unwrap();
    let tmp = fresh(&dir, "open.tmp", b"new");
    show("rename over file open (std sharing)", fs::rename(&tmp, &dest));
    drop(held);
    show("  content after", fs::read_to_string(&dest));

    // The destination memory-mapped, as Brick maps the CSV it shows.
    let dest = fresh(&dir, "mapped.csv", &vec![b'x'; 1 << 16]);
    let map = unsafe { Mmap::map(&File::open(&dest).unwrap()).unwrap() };
    let tmp = fresh(&dir, "mapped.tmp", b"new");
    show("rename over memory-mapped file", fs::rename(&tmp, &dest));
    show("  set_len(0) on memory-mapped file (another program saving in place)",
        OpenOptions::new().write(true).open(&dest).and_then(|f| f.set_len(0)));
    show("  write+truncate open (File::create) on memory-mapped file", File::create(&dest).map(|_| ()));
    println!("  mapping still reads: {}", map.iter().all(|&b| b == b'x'));
    drop(map);
    let _ = fs::remove_file(&tmp);

    // Brick's scratch copies: map, then remove the file while the mapping lives.
    let scratch = fresh(&dir, "scratch.csv", &vec![b'y'; 1 << 16]);
    let map = unsafe { Mmap::map(&File::open(&scratch).unwrap()).unwrap() };
    show("remove memory-mapped file", fs::remove_file(&scratch));
    println!("  exists after: {} ; mapping still reads: {}", scratch.exists(), map.iter().all(|&b| b == b'y'));
    drop(map);
    println!("  exists after unmapping: {}", scratch.exists());

    // Other programs often open without delete sharing (e.g. FILE_SHARE_READ only).
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let dest = fresh(&dir, "locked.csv", b"old");
        let held = OpenOptions::new().read(true).share_mode(1 /* FILE_SHARE_READ */).open(&dest).unwrap();
        let tmp = fresh(&dir, "locked.tmp", b"new");
        show("rename over file open with FILE_SHARE_READ only", fs::rename(&tmp, &dest));
        drop(held);
    }

    // Recovery owner locks: held by one handle, tried by another and by another process.
    let lock = dir.join("owner.lock");
    let owner = File::create(&lock).unwrap();
    show("lock owner", owner.lock());
    let other = File::open(&lock).unwrap();
    println!("try_lock, same process other handle: {:?}", other.try_lock().map_err(|e| e.to_string()));
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["lock-child", lock.to_str().unwrap()])
        .output();
    println!("try_lock, other process: {:?}", child.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()));
    show("read locked file from other handle", fs::read(&lock).map(|b| b.len()));
    show("remove locked file", fs::remove_file(&lock));
    drop(owner);

    // Where the apps' folders land: they read XDG_* and HOME.
    for v in ["HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "XDG_STATE_HOME", "XDG_CACHE_HOME", "XDG_CONFIG_HOME", "TEMP"] {
        println!("env {v}={:?}", std::env::var_os(v));
    }
    #[allow(deprecated)]
    let home = std::env::home_dir();
    println!("std::env::home_dir={home:?} temp_dir={:?}", std::env::temp_dir());
    let state = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state").join("brick");
    // suite_file::recent: canonicalize, then keep only lines starting with '/'.
    let doc = fresh(&dir, "recent.csv", b"x");
    match fs::canonicalize(&doc) {
        Ok(c) => println!(
            "canonicalize={:?} starts with '/': {} is_absolute: {}",
            c,
            c.as_os_str().as_encoded_bytes().first() == Some(&b'/'),
            c.is_absolute()
        ),
        Err(e) => println!("canonicalize: Err {e}"),
    }
    println!("Brick's state dir without XDG_STATE_HOME: {:?} (absolute: {})", state, state.is_absolute());
    println!("/proc/{} exists (Slate's liveness test): {}", std::process::id(), Path::new(&format!("/proc/{}", std::process::id())).exists());

    let mut f = File::create(dir.join("done")).unwrap();
    f.write_all(b"ok").unwrap();
    let _ = fs::remove_dir_all(&dir);
}
