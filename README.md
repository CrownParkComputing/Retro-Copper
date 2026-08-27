<div align="center">

# Retro-Amiga · Copperline Edition

### The Retro-Amiga workbench, rebuilt on the Copperline core.

</div>

---

This is a fork of [Retro-Amiga](https://github.com/CrownParkComputing/Retro-Amiga)
exploring **[Copperline](https://github.com/LinuxJedi/Copperline)** — a
cycle-driven Amiga chipset emulator written in Rust — as the emulation core in
place of Amiberry. The Flutter workbench GUI, wizard, library, session screen
and compliance story carry over unchanged from Retro-Amiga; only the machine
underneath changes.

## Why Copperline

- Rust core: memory safety in exactly the layer that crashes emulators.
- Cycle-driven chipset (copper/blitter/bitplane timing modelled directly),
  including the AGA lo-res 8-bitplane path.
- A codebase we already work in upstream, rather than a vendored snapshot
  that quietly ages.

## Status: the bridge boots

`native/copperline_ffi/` is the bridge - a C ABI cdylib over the HEADLESS
Copperline core, shaped like the surface the project's own wasm frontend
uses. Proven by `smoke.c`: AROS ROM in, A1200/PAL machine up, 121 frames
stepped under wall-clock pacing, a real 668x540 RGBA picture out, 175k audio
samples through Paula.

```sh
git submodule update --init copperline
cd native/copperline_ffi && cargo build --release
cc smoke.c -L target/release -lcopperline_ffi -o smoke
LD_LIBRARY_PATH=target/release ./smoke \
  ../../copperline/assets/aros/aros-amiga-m68k-rom.bin \
  ../../copperline/assets/aros/aros-amiga-m68k-ext.bin
```

The surface: `cl_new(model, video, floppy_drives)` - the Core tab's options
are the same knobs the desktop's `--model` flag takes (A500/A1200/CD32, PAL/
NTSC, 0-4 drives) - then `cl_load_rom`, `cl_run(now_ms, max_frames)`,
`cl_present_ptr/width/rows`, `cl_take_audio`, `cl_key_event` (W3C codes, what
Flutter key events carry), `cl_mouse_*`, `cl_set_joystick`,
`cl_insert_floppy`/`cl_eject_floppy`.

Remaining milestones:

2. Android arm64 build of the cdylib (cargo-ndk), packaged in the app.
3. Flutter FFI bindings + an emulator isolate driving cl_run at 50 Hz,
   presenting through the same texture path the Amiberry build uses.
4. Delete the Amiberry remnants for real; savestates via the core's
   save-state support.

Until then, [Retro-Amiga](https://github.com/CrownParkComputing/Retro-Amiga)
remains the shipping app.

## Working on the core

```sh
git submodule update --init copperline
cd copperline && cargo build --release
```

Keep the submodule moving: `git -C copperline pull origin main` and commit the
bump — the whole point of this fork is never being months behind the core.
