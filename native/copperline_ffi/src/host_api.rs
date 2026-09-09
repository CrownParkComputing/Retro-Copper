// The door the app already knocks on.
//
// The Flutter launcher, the Swift in-game controls and the Android overlay
// all speak one plain-C interface: `uae4arm_host_*`, twenty-five functions
// that Amiberry's src/osdep/uae4arm_host.cpp exports. Nothing about that
// interface is Amiberry-specific - it is run, quit, keys, mouse, pads,
// floppies, a framebuffer and a session - so this module implements it on
// top of Copperline instead.
//
// Doing it this way rather than teaching the front end a second interface
// means the Dart, Swift and Kotlin layers do not change at all, and which
// core the app runs becomes a link-time choice: build this crate as
// `libuae4arm` and Copperline answers, build the C++ tree and Amiberry does.
// A game that misbehaves under one can be checked against the other without
// touching a line of the app.
//
// THREADING. The same contract the cl_* surface has: the app owns one
// emulator thread (a Dart isolate on Android and Linux, the main thread on
// iOS) and every call for the machine comes from it, except the handful the
// UI thread uses to read a frame or push input, which take the lock briefly.

use std::ffi::{CStr, CString};
use std::sync::OnceLock;
use std::os::raw::c_char;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::Mutex;

use crate::uae_config::{session_from, UaeFile};
use crate::ClEmu;

/// The machine, and the little state the host interface keeps around it.
struct Host {
    emu: ClEmu,
    /// Bumped every time a new picture is presented, so the view can tell
    /// whether the frame it holds is stale without comparing pixels.
    serial: u64,
    title: String,
}

/// `ClEmu` holds `Rc`, so it is not `Send`. The contract above is what makes
/// this sound: one thread owns the machine, and the brief readers take the
/// same lock. Stated here rather than left for someone to infer.
struct Owned(Option<Host>);
unsafe impl Send for Owned {}

static HOST: Mutex<Owned> = Mutex::new(Owned(None));

static QUIT: AtomicBool = AtomicBool::new(false);
static PAUSED: AtomicBool = AtomicBool::new(false);
static FRAMEBUFFER_OUTPUT: AtomicBool = AtomicBool::new(true);
static LOGFILE_ENABLED: AtomicBool = AtomicBool::new(false);
static SERIAL: AtomicU64 = AtomicU64::new(0);
static PAD_PORT: AtomicI32 = AtomicI32::new(1);

/// The pending session, set before `run` is called and consumed by it. The
/// app hands over a config path first and then gives the core its thread.
static PENDING: Mutex<Option<(PathBuf, PathBuf, String)>> = Mutex::new(None);

fn string_from(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

fn with_host<T>(f: impl FnOnce(&mut Host) -> T) -> Option<T> {
    let mut guard = HOST.lock().ok()?;
    guard.0.as_mut().map(f)
}

// ---------------------------------------------------------------- session

/// Remember the machine to run: a save-state path, the config file the app
/// wrote, and the title to show. The machine is built when `run` takes over
/// the thread, because building it here would put a whole emulator behind
/// the UI thread's back.
#[no_mangle]
pub extern "C" fn uae4arm_host_set_session(
    state: *const c_char,
    config: *const c_char,
    title: *const c_char,
) {
    let session = (
        PathBuf::from(string_from(state)),
        PathBuf::from(string_from(config)),
        string_from(title),
    );
    if let Ok(mut pending) = PENDING.lock() {
        *pending = Some(session);
    }
}

/// Save state is not wired to Copperline's own savestate yet, and saying so
/// is better than returning true and losing someone's progress.
#[no_mangle]
pub extern "C" fn uae4arm_host_save_session() -> bool {
    false
}

/// The core's entry point: builds the machine the session named and runs it
/// until something asks it to quit. Argc/argv are accepted and ignored - the
/// app passes the library path and its own arguments, and Copperline is
/// configured from the file, not the command line.
#[no_mangle]
pub extern "C" fn uae4arm_host_run(_argc: i32, _argv: *const *const c_char) -> i32 {
    QUIT.store(false, Ordering::SeqCst);

    let pending = PENDING.lock().ok().and_then(|mut p| p.take());
    let Some((_state, config_path, title)) = pending else {
        eprintln!("uae4arm_host_run: no session was set");
        return 1;
    };

    let file = match UaeFile::read(&config_path) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("uae4arm_host_run: {}: {err}", config_path.display());
            return 1;
        }
    };
    let session = match session_from(&file) {
        Ok(session) => session,
        Err(err) => {
            eprintln!("uae4arm_host_run: {}: {err:#}", config_path.display());
            return 1;
        }
    };
    for note in &session.unmapped.0 {
        eprintln!("uae4arm_host_run: not carried across: {note}");
    }

    let emu = match ClEmu::from_config(session.config) {
        Ok(emu) => emu,
        Err(err) => {
            eprintln!("uae4arm_host_run: could not build the machine: {err:#}");
            return 1;
        }
    };
    if let Ok(mut guard) = HOST.lock() {
        guard.0 = Some(Host { emu, serial: SERIAL.load(Ordering::SeqCst), title });
    }
    if let Some(title) = session_title() {
        eprintln!("uae4arm_host_run: running {title} on the Copperline core");
    }

    let start = std::time::Instant::now();
    while !QUIT.load(Ordering::SeqCst) {
        if PAUSED.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(16));
            continue;
        }
        let now_ms = start.elapsed().as_secs_f64() * 1000.0;
        let stepped = with_host(|host| match host.emu.run(now_ms, 4) {
            Ok(frames) => {
                if frames > 0 {
                    host.serial = SERIAL.fetch_add(1, Ordering::SeqCst) + 1;
                }
                Ok(frames)
            }
            Err(err) => Err(format!("{err:#}")),
        });
        match stepped {
            None => break, // the machine went away underneath us
            Some(Err(err)) => {
                eprintln!("uae4arm_host_run: {err}");
                break;
            }
            Some(Ok(0)) => std::thread::sleep(std::time::Duration::from_millis(1)),
            Some(Ok(_)) => {}
        }
    }

    if let Ok(mut guard) = HOST.lock() {
        guard.0 = None;
    }
    0
}

#[no_mangle]
pub extern "C" fn uae4arm_host_quit() {
    QUIT.store(true, Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn uae4arm_host_set_pause(paused: bool) {
    PAUSED.store(paused, Ordering::SeqCst);
}

// ------------------------------------------------------------ the picture

#[no_mangle]
pub extern "C" fn uae4arm_host_set_framebuffer_output(on: bool) {
    FRAMEBUFFER_OUTPUT.store(on, Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn uae4arm_host_framebuffer_serial() -> u64 {
    SERIAL.load(Ordering::SeqCst)
}

/// The view telling the core it has finished with the last frame. Copperline
/// presents into its own buffer and never blocks on the host, so there is
/// nothing to release; the serial is returned so the caller can see which
/// frame it acknowledged.
#[no_mangle]
pub extern "C" fn uae4arm_host_texture_posted() -> u64 {
    SERIAL.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn uae4arm_host_framebuffer_size(width: *mut i32, height: *mut i32) {
    let (w, h) = with_host(|host| {
        (host.emu.present_width() as i32, host.emu.present_rows() as i32)
    })
    .unwrap_or((0, 0));
    unsafe {
        if !width.is_null() {
            *width = w;
        }
        if !height.is_null() {
            *height = h;
        }
    }
}

/// Copy the presented picture out. Returns the number of pixels written, or
/// zero when there is nothing to give: no machine, output switched off, or a
/// destination too small for the frame.
#[no_mangle]
pub extern "C" fn uae4arm_host_copy_framebuffer(
    dst: *mut u32,
    capacity_pixels: i32,
    out_width: *mut i32,
    out_height: *mut i32,
    out_serial: *mut u64,
) -> i32 {
    if dst.is_null() || capacity_pixels <= 0 || !FRAMEBUFFER_OUTPUT.load(Ordering::SeqCst) {
        return 0;
    }
    let copied = with_host(|host| {
        let width = host.emu.present_width() as usize;
        let rows = host.emu.present_rows() as usize;
        let pixels = width * rows;
        if pixels == 0 || pixels > capacity_pixels as usize {
            return (0usize, width, rows, host.serial);
        }
        let src = host.emu.present_pixels();
        unsafe {
            std::ptr::copy_nonoverlapping(src.as_ptr(), dst, pixels);
        }
        (pixels, width, rows, host.serial)
    });
    let (pixels, width, rows, serial) = copied.unwrap_or((0, 0, 0, 0));
    unsafe {
        if !out_width.is_null() {
            *out_width = width as i32;
        }
        if !out_height.is_null() {
            *out_height = rows as i32;
        }
        if !out_serial.is_null() {
            *out_serial = serial;
        }
    }
    pixels as i32
}

// -------------------------------------------------------------- the input

/// Amiga raw keycodes in, the same numbers the on-screen keyboard sends.
#[no_mangle]
pub extern "C" fn uae4arm_host_send_key(amiga_keycode: i32, pressed: bool) {
    with_host(|host| host.emu.send_amiga_key(amiga_keycode, pressed));
}

#[no_mangle]
pub extern "C" fn uae4arm_host_mouse_move(dx: i32, dy: i32) {
    with_host(|host| host.emu.mouse_delta_counts(dx, dy));
}

#[no_mangle]
pub extern "C" fn uae4arm_host_mouse_button(button: i32, pressed: bool) {
    with_host(|host| host.emu.mouse_button(button.clamp(0, 2) as u8, pressed));
}

// The pads. Amiberry registers a virtual pad as an input device so the core
// cannot tell it from a real one; Copperline takes joystick state per port
// directly, so attach and release are bookkeeping and only direction and
// buttons reach the machine.

#[no_mangle]
pub extern "C" fn uae4arm_host_pad_attach(port: i32) {
    PAD_PORT.store(port.clamp(0, 1), Ordering::SeqCst);
}

/// Copperline takes a whole port at once, while the app pushes directions
/// and buttons separately, so the current state of each port is kept here
/// and pushed in full on every change.
#[derive(Clone, Copy, Default)]
struct Pad {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    fire: bool,
    second: bool,
}

static PADS: Mutex<[Pad; 2]> = Mutex::new([Pad { up: false, down: false, left: false, right: false, fire: false, second: false }; 2]);

fn push_pad(port: u8) {
    let Ok(pads) = PADS.lock() else { return };
    let pad = pads[port as usize];
    drop(pads);
    with_host(|host| {
        host.emu
            .set_joystick(port, pad.up, pad.down, pad.left, pad.right, pad.fire, pad.second)
    });
}

#[no_mangle]
pub extern "C" fn uae4arm_host_pad_direction(
    port: i32,
    up: bool,
    down: bool,
    left: bool,
    right: bool,
) {
    let port = port.clamp(0, 1) as u8;
    if let Ok(mut pads) = PADS.lock() {
        let pad = &mut pads[port as usize];
        pad.up = up;
        pad.down = down;
        pad.left = left;
        pad.right = right;
    }
    push_pad(port);
}

#[no_mangle]
pub extern "C" fn uae4arm_host_pad_button(port: i32, button: i32, pressed: bool) {
    let port = port.clamp(0, 1) as u8;
    if let Ok(mut pads) = PADS.lock() {
        let pad = &mut pads[port as usize];
        match button {
            0 => pad.fire = pressed,
            1 => pad.second = pressed,
            _ => {}
        }
    }
    push_pad(port);
}

#[no_mangle]
pub extern "C" fn uae4arm_host_pad_release_all(port: i32) {
    let port = port.clamp(0, 1) as u8;
    if let Ok(mut pads) = PADS.lock() {
        pads[port as usize] = Pad::default();
    }
    push_pad(port);
}

#[no_mangle]
pub extern "C" fn uae4arm_host_pad_port() -> i32 {
    PAD_PORT.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn uae4arm_host_swap_pad_port() {
    let port = PAD_PORT.load(Ordering::SeqCst);
    PAD_PORT.store(1 - port, Ordering::SeqCst);
}

/// Which on-screen control surface the app is showing, and whether a real
/// controller has taken over. Copperline reads pad state per port and does
/// not care where it came from, so these are recorded for `pad_port` and
/// otherwise have nothing to do.
#[no_mangle]
pub extern "C" fn uae4arm_host_set_onscreen_controller(_mode: i32) {}

#[no_mangle]
pub extern "C" fn uae4arm_host_set_external_controller_mode(_mode: i32) {}

// ------------------------------------------------------------- the floppies

#[no_mangle]
pub extern "C" fn uae4arm_host_insert_floppy(drive: i32, path: *const c_char) {
    let path = string_from(path);
    with_host(|host| {
        if path.trim().is_empty() {
            host.emu.eject_floppy(drive.clamp(0, 3) as u8);
        } else {
            if let Err(err) = host.emu.insert_floppy_path(drive.clamp(0, 3) as u8, &path) {
                eprintln!("uae4arm_host_insert_floppy: {path}: {err:#}");
            }
        }
    });
}

#[no_mangle]
pub extern "C" fn uae4arm_host_get_floppy_count() -> i32 {
    with_host(|host| host.emu.floppy_count() as i32).unwrap_or(0)
}

// ---------------------------------------------------------------- logging

#[no_mangle]
pub extern "C" fn uae4arm_host_set_logfile_enabled(enabled: bool) {
    LOGFILE_ENABLED.store(enabled, Ordering::SeqCst);
}

/// Where the core writes its log. Copperline logs to stderr, which the
/// platform captures already, so there is no file to name. Returning an
/// empty string rather than null keeps the Dart side's Utf8 conversion safe.
#[no_mangle]
pub extern "C" fn uae4arm_host_logfile_path() -> *const c_char {
    static EMPTY: &[u8] = b"\0";
    EMPTY.as_ptr() as *const c_char
}

/// Which core answered. Amiberry's host library has no such export, so the
/// app looks this up and treats a miss as "Amiberry": the two builds are
/// otherwise indistinguishable through this interface, which is the point,
/// and an About screen that names the wrong emulator is the kind of small
/// untruth that turns into a store rejection.
#[no_mangle]
pub extern "C" fn uae4arm_host_core_name() -> *const c_char {
    static NAME: OnceLock<CString> = OnceLock::new();
    NAME.get_or_init(|| {
        CString::new(format!("Copperline {}", copperline_version())).unwrap()
    })
    .as_ptr()
}

fn copperline_version() -> &'static str {
    option_env!("COPPERLINE_VERSION").unwrap_or("0.17")
}

/// The title the app gave the session. Printed when the machine comes up so
/// a device log says which game a run belongs to.
pub(crate) fn session_title() -> Option<String> {
    with_host(|host| host.title.clone())
}
