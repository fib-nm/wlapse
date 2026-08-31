use std::ffi::OsStr;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Colors {
    pub background: u32,
    pub text: u32,
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            background: 0xD91B_1D23,
            text: 0xFFFF_FFFF,
        }
    }
}

pub fn config_path(xdg_config_home: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    xdg_config_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| home.join(".config"))
        })
        .map(|base| base.join("wlapse/config"))
}

pub fn load(path: Option<&Path>) -> io::Result<Colors> {
    let Some(path) = path else {
        return Ok(Colors::default());
    };
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Colors::default()),
        Err(error) => return Err(error),
    };

    let mut colors = Colors::default();
    let mut has_background = false;
    let mut has_text = false;
    for (index, line) in contents.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| {
            invalid_config(index + 1, "expected a key and value separated by '='")
        })?;
        let key = key.trim();
        let duplicate = match key {
            "background_color" => &mut has_background,
            "text_color" => &mut has_text,
            key => return Err(invalid_config(index + 1, format!("unknown key '{key}'"))),
        };
        if *duplicate {
            return Err(invalid_config(index + 1, format!("duplicate key '{key}'")));
        }
        *duplicate = true;

        let color =
            parse_color(value.trim()).map_err(|message| invalid_config(index + 1, message))?;
        match key {
            "background_color" => colors.background = color,
            "text_color" => colors.text = color,
            _ => unreachable!(),
        }
    }
    Ok(colors)
}

fn parse_color(value: &str) -> Result<u32, &'static str> {
    let hex = value.strip_prefix('#').ok_or("color must start with '#'")?;
    if hex.len() != 6 && hex.len() != 8 {
        return Err("color must use #RRGGBB or #RRGGBBAA");
    }
    if !hex.as_bytes().iter().all(u8::is_ascii_hexdigit) {
        return Err("color contains a non-hexadecimal digit");
    }
    let channel = |start| {
        u8::from_str_radix(&hex[start..start + 2], 16)
            .map_err(|_| "color contains a non-hexadecimal digit")
    };
    let red = channel(0)?;
    let green = channel(2)?;
    let blue = channel(4)?;
    let alpha = if hex.len() == 8 { channel(6)? } else { 255 };
    let premultiply = |component: u8| (u32::from(component) * u32::from(alpha) + 127) / 255;

    Ok((u32::from(alpha) << 24)
        | (premultiply(red) << 16)
        | (premultiply(green) << 8)
        | premultiply(blue))
}

fn invalid_config(line: usize, message: impl Into<String>) -> io::Error {
    io::Error::new(
        ErrorKind::InvalidData,
        format!("invalid config at line {line}: {}", message.into()),
    )
}
