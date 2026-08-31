use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use wlapse::config::{Colors, config_path, load};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "wlapse-config-test-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).expect("create temporary config directory");
    path
}

#[test]
fn config_path_prefers_xdg_config_home_and_falls_back_to_home() {
    assert_eq!(
        config_path(Some("/xdg".as_ref()), Some("/home/test".as_ref())),
        Some(PathBuf::from("/xdg/wlapse/config"))
    );
    assert_eq!(
        config_path(None, Some("/home/test".as_ref())),
        Some(PathBuf::from("/home/test/.config/wlapse/config"))
    );
    assert_eq!(
        config_path(Some("relative".as_ref()), Some("also-relative".as_ref())),
        None
    );
}

#[test]
fn missing_config_uses_default_colors() {
    let directory = temp_dir();
    let path = directory.join("missing");

    assert_eq!(
        load(Some(&path)).expect("load defaults"),
        Colors {
            background: 0xD91B_1D23,
            text: 0xFFFF_FFFF,
        }
    );

    fs::remove_dir(directory).expect("remove temporary config directory");
}

#[test]
fn documented_background_color_matches_the_absent_config_default() {
    let directory = temp_dir();
    let path = directory.join("config");
    fs::write(&path, "background_color = #202229d9\n").expect("write config");

    assert_eq!(load(Some(&path)).expect("load colors"), Colors::default());

    fs::remove_dir_all(directory).expect("remove temporary config directory");
}

#[test]
fn config_overrides_background_and_text_colors() {
    let directory = temp_dir();
    let path = directory.join("config");
    fs::write(
        &path,
        "background_color = #33669980\ntext_color = #abcdef\n",
    )
    .expect("write config");

    assert_eq!(
        load(Some(&path)).expect("load colors"),
        Colors {
            background: 0x801A_334D,
            text: 0xFFAB_CDEF,
        }
    );

    fs::remove_dir_all(directory).expect("remove temporary config directory");
}

#[test]
fn duplicate_color_key_is_rejected() {
    let directory = temp_dir();
    let path = directory.join("config");
    fs::write(&path, "text_color = #ffffff\ntext_color = #000000\n").expect("write config");

    let error = load(Some(&path)).expect_err("reject duplicate key");
    assert_eq!(
        error.to_string(),
        "invalid config at line 2: duplicate key 'text_color'"
    );

    fs::remove_dir_all(directory).expect("remove temporary config directory");
}

#[test]
fn non_ascii_color_is_rejected_without_panicking() {
    let directory = temp_dir();
    let path = directory.join("config");
    fs::write(&path, "background_color = #aéabc\n").expect("write config");

    let error = load(Some(&path)).expect_err("reject non-ASCII color");
    assert_eq!(
        error.to_string(),
        "invalid config at line 1: color contains a non-hexadecimal digit"
    );

    fs::remove_dir_all(directory).expect("remove temporary config directory");
}
