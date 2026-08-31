use rustix::fs::{Mode, OFlags, open};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub fn state_path(xdg_state_home: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    xdg_state_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| home.join(".local/state"))
        })
        .map(|base| base.join("wlapse/placement"))
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

const MAX_STATE_BYTES: u64 = 64;

#[derive(Debug)]
pub struct PlacementStore {
    path: PathBuf,
}

impl PlacementStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> io::Result<Position> {
        let fd = match open(
            &self.path,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::NOFOLLOW,
            Mode::empty(),
        ) {
            Ok(fd) => fd,
            Err(rustix::io::Errno::NOENT | rustix::io::Errno::LOOP) => {
                return Ok(Position::default());
            }
            Err(error) => return Err(io::Error::from(error)),
        };
        let file = File::from(fd);
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_STATE_BYTES {
            return Ok(Position::default());
        }

        let mut bytes = Vec::with_capacity(MAX_STATE_BYTES as usize + 1);
        file.take(MAX_STATE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_STATE_BYTES as usize {
            return Ok(Position::default());
        }
        let Ok(contents) = std::str::from_utf8(&bytes) else {
            return Ok(Position::default());
        };
        match parse_position(contents) {
            Ok(position) => Ok(position),
            Err(error) if error.kind() == ErrorKind::InvalidData => Ok(Position::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, position: Position) -> io::Result<()> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let temporary = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        match fs::remove_file(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            writeln!(file, "{} {}", position.x, position.y)?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)?;
            FileSync::directory(parent)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

struct FileSync;

impl FileSync {
    fn directory(path: &Path) -> io::Result<()> {
        fs::File::open(path)?.sync_all()
    }
}

fn parse_position(contents: &str) -> io::Result<Position> {
    let mut fields = contents.split_whitespace();
    let x = fields
        .next()
        .ok_or_else(invalid_position)?
        .parse::<i32>()
        .map_err(|_| invalid_position())?;
    let y = fields
        .next()
        .ok_or_else(invalid_position)?
        .parse::<i32>()
        .map_err(|_| invalid_position())?;
    if fields.next().is_some() || x < 0 || y < 0 {
        return Err(invalid_position());
    }
    Ok(Position { x, y })
}

fn invalid_position() -> io::Error {
    io::Error::new(ErrorKind::InvalidData, "invalid saved placement")
}

#[derive(Debug)]
pub struct Placement {
    drag: Drag,
    store: Option<PlacementStore>,
    dirty: bool,
}

impl Placement {
    pub fn new(position: Position, store: Option<PlacementStore>) -> Self {
        Self {
            drag: Drag::new(position),
            store,
            dirty: false,
        }
    }

    pub fn press(&mut self) {
        self.drag.press();
    }

    pub fn motion(&mut self, dx: f64, dy: f64) -> Option<Position> {
        let position = self.drag.motion(dx, dy)?;
        self.dirty = true;
        Some(position)
    }

    pub fn release(&mut self) -> io::Result<()> {
        self.drag.release();
        self.save_if_dirty()
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        self.drag.release();
        self.save_if_dirty()
    }

    pub fn position(&self) -> Position {
        self.drag.position()
    }

    fn save_if_dirty(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(store) = self.store.as_ref() {
            store.save(self.drag.position())?;
        }
        self.dirty = false;
        Ok(())
    }
}

#[derive(Debug)]
pub struct Drag {
    position_x: f64,
    position_y: f64,
    active: bool,
    moved: bool,
}

impl Drag {
    pub fn new(position: Position) -> Self {
        Self {
            position_x: f64::from(position.x),
            position_y: f64::from(position.y),
            active: false,
            moved: false,
        }
    }

    pub fn press(&mut self) {
        self.active = true;
        self.moved = false;
    }

    pub fn motion(&mut self, dx: f64, dy: f64) -> Option<Position> {
        if !self.active || !dx.is_finite() || !dy.is_finite() {
            return None;
        }
        self.position_x = (self.position_x + dx).clamp(0.0, f64::from(i32::MAX));
        self.position_y = (self.position_y + dy).clamp(0.0, f64::from(i32::MAX));
        self.moved = true;
        Some(self.position())
    }

    pub fn release(&mut self) -> Option<Position> {
        if !self.active {
            return None;
        }
        self.active = false;
        self.moved.then(|| self.position())
    }

    pub fn position(&self) -> Position {
        Position {
            x: self.position_x.round() as i32,
            y: self.position_y.round() as i32,
        }
    }
}
