// Reading the front end's own machine configuration.
//
// The Flutter app has always written an Amiberry-style `.uae` file: one
// `key=value` per line, produced by app/lib/data/config_generator.dart. That
// file is where a collection lives - `filesystem2=` lines for AGS and
// AmigaVision directory mounts, `hardfile2=` lines for HDF images - so
// nothing bigger than a floppy can reach the guest until it is understood.
//
// Copperline has its own TOML configuration and ships a converter for UAE
// files, but that converter lives under src/bin/ in the upstream repository
// and is not part of the library. Rather than fork upstream or vendor two
// thousand lines of a mapper built for every WinUAE key ever written, this
// maps the fifty-odd keys OUR OWN writer emits. The set is small because we
// control both ends, and anything unmapped is reported rather than ignored.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use copperline::config::{
    machine_profile_defaults, parse_machine_model, parse_video_standard, Config, DriveImage,
};
use copperline::filesys::MountSpec;

/// One `key=value` file, parsed. Later lines win, which is how UAE itself
/// treats a repeated key.
#[derive(Debug, Default)]
pub struct UaeFile {
    values: BTreeMap<String, String>,
    /// `filesystem2` and `hardfile2` may appear many times over, so they are
    /// kept in order rather than folded into the map above.
    filesystems: Vec<String>,
    hardfiles: Vec<String>,
}

impl UaeFile {
    pub fn parse(text: &str) -> UaeFile {
        let mut out = UaeFile::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "filesystem2" => out.filesystems.push(value.to_string()),
                "hardfile2" => out.hardfiles.push(value.to_string()),
                _ => {
                    out.values.insert(key.to_ascii_lowercase(), value.to_string());
                }
            }
        }
        out
    }

    pub fn read(path: &Path) -> std::io::Result<UaeFile> {
        Ok(UaeFile::parse(&std::fs::read_to_string(path)?))
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|s| s.as_str())
    }

    fn int(&self, key: &str) -> Option<i64> {
        self.get(key)?.trim().parse().ok()
    }

    fn bool(&self, key: &str) -> Option<bool> {
        match self.get(key)?.trim() {
            "true" | "yes" | "1" => Some(true),
            "false" | "no" | "0" => Some(false),
            _ => None,
        }
    }
}

/// A directory handed to the guest as a volume: `filesystem2=` .
#[derive(Debug, PartialEq, Eq)]
pub struct DirMount {
    pub device: String,
    pub volume: String,
    pub path: PathBuf,
    pub boot_pri: i8,
    pub readonly: bool,
}

/// An image handed to the guest as a disk: `hardfile2=` .
#[derive(Debug, PartialEq, Eq)]
pub struct ImageMount {
    pub device: String,
    pub path: PathBuf,
    pub boot_pri: i8,
    pub readonly: bool,
    /// `uae`, `ide0`, `scsi0` and so on - which controller the writer asked
    /// for. Copperline models real controllers, so `uae` becomes an IDE unit.
    pub controller: String,
}

/// `rw,DH0:Work:/path/to/dir,0` - access, then device:volume:path, then the
/// boot priority. The path may be quoted, and may itself contain a comma,
/// which is why this walks the string rather than splitting on commas.
fn parse_filesystem2(spec: &str) -> Option<DirMount> {
    let (access, rest) = spec.split_once(',')?;
    let readonly = access.trim().eq_ignore_ascii_case("ro");
    let (device, rest) = rest.split_once(':')?;
    let (volume, rest) = rest.split_once(':')?;
    let (path, tail) = take_path(rest);
    let boot_pri = tail
        .trim_start_matches(',')
        .split(',')
        .next()
        .and_then(|v| v.trim().parse::<i8>().ok())
        .unwrap_or(0);
    Some(DirMount {
        device: device.trim().to_string(),
        volume: volume.trim().to_string(),
        path: PathBuf::from(path),
        boot_pri,
        readonly,
    })
}

/// `rw,DH0:"/path/to.hdf",32,1,2,512,0,,ide0` - access, device, quoted path,
/// then geometry, block size, boot priority, filesystem, controller. Only the
/// path, priority and controller matter here: Copperline reads an image's own
/// RDB rather than being told its shape.
fn parse_hardfile2(spec: &str) -> Option<ImageMount> {
    let (access, rest) = spec.split_once(',')?;
    let readonly = access.trim().eq_ignore_ascii_case("ro");
    let (device, rest) = rest.split_once(':')?;
    let (path, tail) = take_path(rest);
    let fields: Vec<&str> = tail.trim_start_matches(',').split(',').collect();
    // Geometry is three fields (surfaces, reserved, sectors) then the block
    // size, and the boot priority follows. A writer that omits the geometry
    // leaves the priority in the first slot instead, so take the last field
    // that parses as a priority before the controller name.
    let boot_pri = fields
        .iter()
        .take(6)
        .filter_map(|f| f.trim().parse::<i8>().ok())
        .last()
        .unwrap_or(0);
    let controller = fields
        .iter()
        .rev()
        .map(|f| f.trim())
        .find(|f| !f.is_empty() && f.parse::<i64>().is_err())
        .unwrap_or("uae")
        .to_string();
    Some(ImageMount {
        device: device.trim().to_string(),
        path: PathBuf::from(path),
        boot_pri,
        readonly,
        controller,
    })
}

/// A path that may be quoted. Returns the path and whatever followed it.
fn take_path(rest: &str) -> (String, &str) {
    let rest = rest.trim_start();
    if let Some(body) = rest.strip_prefix('"') {
        if let Some(end) = body.find('"') {
            return (body[..end].to_string(), &body[end + 1..]);
        }
    }
    match rest.find(',') {
        Some(at) => (rest[..at].trim().to_string(), &rest[at..]),
        None => (rest.trim().to_string(), ""),
    }
}

/// What the file asked for that this mapper does not carry across. Reported
/// rather than dropped: a setting that silently does nothing is how an
/// emulator earns a reputation for being haunted.
#[derive(Debug, Default)]
pub struct Unmapped(pub Vec<String>);

/// The machine the file describes. `dirs` and `images` are kept beside the
/// config they were folded into: the host API reports them, and a test can
/// check what was understood without unpicking a built machine.
pub struct Session {
    pub config: Config,
    #[allow(dead_code)]
    pub dirs: Vec<DirMount>,
    #[allow(dead_code)]
    pub images: Vec<ImageMount>,
    pub unmapped: Unmapped,
}

/// UAE memory sizes are all unit counts rather than bytes, and each key uses
/// a different unit. These are the units WinUAE and Amiberry both write.
fn chip_bytes(units: i64) -> usize {
    (units.max(0) as usize) * 512 * 1024
}
fn bogo_bytes(units: i64) -> usize {
    (units.max(0) as usize) * 256 * 1024
}
fn megabytes(units: i64) -> usize {
    (units.max(0) as usize) * 1024 * 1024
}

/// Which machine profile to start from. The writer does not name a model, so
/// it is inferred the way a person would: the CPU and the chipset together.
fn model_for(file: &UaeFile) -> &'static str {
    let cpu = file.int("cpu_model").unwrap_or(68000);
    let chipset = file.get("chipset").unwrap_or("ocs").to_ascii_lowercase();
    if chipset.contains("aga") {
        if cpu >= 68030 {
            "A4000"
        } else {
            "A1200"
        }
    } else if chipset.contains("ecs") {
        "A600"
    } else {
        "A500"
    }
}

pub fn session_from(file: &UaeFile) -> anyhow::Result<Session> {
    let model = model_for(file);
    let mut config = machine_profile_defaults(
        parse_machine_model(model).map_err(anyhow::Error::msg)?,
    );
    let mut unmapped = Unmapped::default();

    if let Some(rom) = file.get("kickstart_rom_file") {
        if !rom.trim().is_empty() {
            config.rom_path = PathBuf::from(rom.trim());
        }
    }
    if file.bool("ntsc").unwrap_or(false) {
        config.video_standard =
            parse_video_standard("NTSC").map_err(anyhow::Error::msg)?;
    }

    if let Some(units) = file.int("chipmem_size") {
        config.chip_ram_bytes = chip_bytes(units);
    }
    if let Some(units) = file.int("bogomem_size") {
        config.slow_ram_bytes = bogo_bytes(units);
    }
    // The two fast-RAM keys are different buses and must not be merged: a
    // Zorro II board autoconfigures only at 64K to 8M, so folding a 256MB
    // Zorro III pool into it fails to build a machine at all.
    if let Some(mb) = file.int("fastmem_size") {
        if mb > 0 {
            config.fast_ram_bytes = megabytes(mb.min(8));
        }
    }
    if let Some(mb) = file.int("z3mem_size") {
        if mb > 0 {
            config.z3_ram_bytes = megabytes(mb);
        }
    }

    let drives = file.int("nr_floppies").unwrap_or(1).clamp(0, 4) as usize;
    config.floppy_connected = std::array::from_fn(|drive| drive < drives);
    for drive in 0..4 {
        if let Some(path) = file.get(&format!("floppy{drive}")) {
            if !path.trim().is_empty() {
                // One disk is a playlist of one: the same field carries a
                // multi-disk game's swap list.
                config.floppy_playlists[drive] = vec![PathBuf::from(path.trim())];
            }
        }
    }

    // A mount whose path has gone is dropped rather than passed on. The core
    // refuses to build a machine around a missing directory, and on a phone
    // that is a routine event: iOS moves the container on every install, and
    // a card can be pulled. Failing to start at all would turn a missing
    // folder into an app that never opens, so it is reported instead.
    let mut dirs: Vec<DirMount> = Vec::new();
    for spec in &file.filesystems {
        let Some(mount) = parse_filesystem2(spec) else {
            unmapped.0.push(format!("filesystem2={spec} could not be read"));
            continue;
        };
        if mount.path.is_dir() {
            dirs.push(mount);
        } else {
            unmapped.0.push(format!("{} is not on this device", mount.path.display()));
        }
    }
    let mut images: Vec<ImageMount> = Vec::new();
    for spec in &file.hardfiles {
        let Some(image) = parse_hardfile2(spec) else {
            unmapped.0.push(format!("hardfile2={spec} could not be read"));
            continue;
        };
        if image.path.is_file() {
            images.push(image);
        } else {
            unmapped.0.push(format!("{} is not on this device", image.path.display()));
        }
    }

    // Directory mounts go to the host filesystem handler: the guest sees a
    // live volume backed by the folder, which is what an AGS or AmigaVision
    // tree on a card actually is. Images go on the IDE controller Copperline
    // models in hardware; the UAE "uae" controller has no counterpart, and
    // the first free IDE slot is where its importer puts one too.
    config.filesys = dirs
        .iter()
        .map(|dir| MountSpec {
            path: dir.path.clone(),
            volume: dir.volume.clone(),
            boot_pri: dir.boot_pri,
            readonly: dir.readonly,
        })
        .collect();

    let mut slots: Vec<&mut Option<DriveImage>> =
        vec![&mut config.ide.master, &mut config.ide.slave];
    for (index, image) in images.iter().enumerate() {
        let Some(slot) = slots.get_mut(index) else {
            unmapped.0.push(format!(
                "hardfile2 {} has no controller slot left (IDE holds two)",
                image.path.display()
            ));
            continue;
        };
        **slot = Some(DriveImage {
            path: image.path.clone(),
            boot_pri: image.boot_pri,
            ..DriveImage::default()
        });
    }

    for key in ["cachesize", "cpu_speed", "gfxcard_size", "whdload_filename"] {
        if let Some(value) = file.get(key) {
            if !value.trim().is_empty() && value.trim() != "0" {
                unmapped.0.push(format!("{key}={value}"));
            }
        }
    }

    Ok(Session { config, dirs, images, unmapped })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape config_generator.dart writes for an AGS collection on an
    /// 040 with a graphics card: two directory mounts and one hardfile.
    const COLLECTION: &str = r#"
chipset=aga
cpu_model=68040
fpu_model=68040
chipmem_size=4
bogomem_size=0
fastmem_size=8
z3mem_size=256
nr_floppies=1
floppy0=/media/df0.adf
kickstart_rom_file=/roms/kick31.rom
filesystem2=rw,DH0:AGS:/media/Amiga/AGS,0
filesystem2=ro,DH1:Games:"/media/Amiga/Games, extra",-1
hardfile2=rw,DH2:"/media/Amiga/work.hdf",32,1,2,512,0,,ide0
ntsc=false
"#;

    /// Writes the collection out with paths that really exist under `root`,
    /// because a mount that is not there is deliberately dropped.
    fn collection_on_disk(root: &Path) -> String {
        std::fs::create_dir_all(root.join("AGS")).unwrap();
        std::fs::create_dir_all(root.join("Games, extra")).unwrap();
        std::fs::write(root.join("work.hdf"), [0u8; 512]).unwrap();
        COLLECTION
            .replace("/media/Amiga/AGS", root.join("AGS").to_str().unwrap())
            .replace(
                "/media/Amiga/Games, extra",
                root.join("Games, extra").to_str().unwrap(),
            )
            .replace("/media/Amiga/work.hdf", root.join("work.hdf").to_str().unwrap())
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("copperline_ffi_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_a_collection() {
        let root = scratch("collection");
        let file = UaeFile::parse(&collection_on_disk(&root));
        let session = session_from(&file).expect("maps");

        assert_eq!(session.config.chip_ram_bytes, 2 * 1024 * 1024);
        // Zorro II and Zorro III are separate pools, not one number.
        assert_eq!(session.config.fast_ram_bytes, 8 * 1024 * 1024);
        assert_eq!(session.config.z3_ram_bytes, 256 * 1024 * 1024);
        assert_eq!(session.config.rom_path, PathBuf::from("/roms/kick31.rom"));

        assert_eq!(session.dirs.len(), 2, "both directory mounts survive");
        assert_eq!(session.dirs[0].volume, "AGS");
        assert_eq!(session.dirs[0].path, root.join("AGS"));
        assert!(!session.dirs[0].readonly);
        // A quoted path containing a comma must not be split on it.
        assert_eq!(session.dirs[1].path, root.join("Games, extra"));
        assert!(session.dirs[1].readonly);
        assert_eq!(session.dirs[1].boot_pri, -1);

        assert_eq!(session.images.len(), 1);
        assert_eq!(session.images[0].path, root.join("work.hdf"));
        assert_eq!(session.images[0].controller, "ide0");

        // The point of all this: the mounts reach the machine, not just the
        // parse result.
        assert_eq!(session.config.filesys.len(), 2);
        assert_eq!(session.config.filesys[0].volume, "AGS");
        assert_eq!(session.config.filesys[0].path, root.join("AGS"));
        assert!(session.config.filesys[1].readonly);
        let master = session.config.ide.master.as_ref().expect("hardfile on IDE");
        assert_eq!(master.path, root.join("work.hdf"));
        assert!(session.config.ide.slave.is_none());
    }

    #[test]
    fn a_third_image_is_reported_rather_than_dropped() {
        let root = scratch("three_images");
        let mut text = String::from("chipset=aga\ncpu_model=68020\n");
        for unit in 0..3 {
            let image = root.join(format!("disk{unit}.hdf"));
            std::fs::write(&image, [0u8; 512]).unwrap();
            text.push_str(&format!(
                "hardfile2=rw,DH{unit}:\"{}\",32,1,2,512,0,,ide0\n",
                image.display()
            ));
        }
        let session = session_from(&UaeFile::parse(&text)).expect("maps");
        assert!(session.config.ide.master.is_some());
        assert!(session.config.ide.slave.is_some());
        assert!(
            session.unmapped.0.iter().any(|note| note.contains("disk2.hdf")),
            "the third image is named in the report: {:?}",
            session.unmapped.0
        );
    }

    /// The mapping is only worth anything if the core accepts it. Built with
    /// the ROM optional, the way the bridge does when the launcher supplies
    /// the Kickstart afterwards, and with mounts that do not exist on this
    /// host: a machine must still come up, because the guest is what fails to
    /// find a missing volume, not the constructor.
    #[test]
    fn the_core_accepts_a_mapped_collection() {
        use copperline::audio::AudioSink;

        struct Silent;
        impl AudioSink for Silent {
            fn push(&mut self, _left: f32, _right: f32) {}
            fn flush(&mut self) {}
        }

        let root = scratch("build");
        let session =
            session_from(&UaeFile::parse(&collection_on_disk(&root))).expect("maps");
        copperline::emulator::build_machine(&session.config, Box::new(Silent), false, true)
            .expect("the core builds the machine this file describes");
    }

    #[test]
    fn a_bare_floppy_machine_still_maps() {
        let file = UaeFile::parse("chipset=ocs\ncpu_model=68000\nnr_floppies=2\n");
        let session = session_from(&file).expect("maps");
        assert!(session.dirs.is_empty());
        assert!(session.images.is_empty());
        assert_eq!(session.config.floppy_connected[1], true);
        assert_eq!(session.config.floppy_connected[2], false);
    }
}
