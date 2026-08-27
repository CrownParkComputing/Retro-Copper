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

## Status: scaffold

The core is vendored as the `copperline/` submodule, pinned to upstream
`main`. **The app does not build yet** — the first milestone is the bridge:

1. Build Copperline as a `cdylib` for Android (arm64) and Linux.
2. A C ABI surface mirroring what the Dart side already speaks to Amiberry:
   boot/insert/eject, run/pause, framebuffer handout, audio ring, input
   injection, savestates.
3. Point the Flutter FFI bindings at the new library and delete the Amiberry
   remnants for real.

Until then, [Retro-Amiga](https://github.com/CrownParkComputing/Retro-Amiga)
remains the shipping app.

## Working on the core

```sh
git submodule update --init copperline
cd copperline && cargo build --release
```

Keep the submodule moving: `git -C copperline pull origin main` and commit the
bump — the whole point of this fork is never being months behind the core.
