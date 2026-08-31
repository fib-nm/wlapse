use std::ffi::OsStr;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use wlapse::placement::{Drag, Placement, PlacementStore, Position, state_path};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn temporary_directory() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "wlapse-placement-test-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).expect("create temporary directory");
    path
}

#[test]
fn drag_accumulates_relative_motion_from_any_press_point() {
    let mut drag = Drag::new(Position { x: 100, y: 50 });

    drag.press();
    assert_eq!(drag.motion(4.4, -2.6), Some(Position { x: 104, y: 47 }));
    assert_eq!(drag.motion(0.7, 1.2), Some(Position { x: 105, y: 49 }));
    assert_eq!(drag.release(), Some(Position { x: 105, y: 49 }));
}

#[test]
fn motion_is_ignored_outside_an_active_drag() {
    let mut drag = Drag::new(Position { x: 20, y: 30 });

    assert_eq!(drag.motion(10.0, 10.0), None);
    assert_eq!(drag.release(), None);
    assert_eq!(drag.position(), Position { x: 20, y: 30 });
}

#[test]
fn non_finite_relative_motion_is_ignored() {
    let mut drag = Drag::new(Position { x: 20, y: 30 });
    drag.press();

    assert_eq!(drag.motion(f64::NAN, 1.0), None);
    assert_eq!(drag.motion(1.0, f64::INFINITY), None);
    assert_eq!(drag.position(), Position { x: 20, y: 30 });
    assert_eq!(drag.release(), None);
}

#[test]
fn placement_round_trips_through_an_atomic_state_file() {
    let directory = temporary_directory();
    let path = directory.join("placement");
    let store = PlacementStore::new(path.clone());

    assert_eq!(
        store.load().expect("load missing state"),
        Position::default()
    );
    store
        .save(Position { x: 712, y: 39 })
        .expect("save placement");
    assert_eq!(
        store.load().expect("load saved placement"),
        Position { x: 712, y: 39 }
    );
    assert_eq!(fs::read_to_string(&path).expect("read state"), "712 39\n");

    fs::remove_dir_all(directory).expect("remove temporary directory");
}

#[test]
fn stale_temporary_file_does_not_prevent_saving() {
    let directory = temporary_directory();
    let path = directory.join("placement");
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temporary, "stale").expect("write stale temporary file");
    let store = PlacementStore::new(path);

    store
        .save(Position { x: 9, y: 11 })
        .expect("replace stale temporary file");
    assert_eq!(
        store.load().expect("load placement"),
        Position { x: 9, y: 11 }
    );

    fs::remove_dir_all(directory).expect("remove temporary directory");
}

#[test]
fn oversized_or_symlinked_state_is_ignored() {
    use std::os::unix::fs::symlink;

    let directory = temporary_directory();
    let path = directory.join("placement");
    let store = PlacementStore::new(path.clone());

    fs::write(&path, format!("9 11{}", " ".repeat(128))).expect("write oversized state");
    assert_eq!(
        store.load().expect("load oversized state"),
        Position::default()
    );

    fs::remove_file(&path).expect("remove oversized state");
    let target = directory.join("target");
    fs::write(&target, "9 11\n").expect("write symlink target");
    symlink(&target, &path).expect("create state symlink");
    assert_eq!(
        store.load().expect("load symlink state"),
        Position::default()
    );

    fs::remove_dir_all(directory).expect("remove temporary directory");
}

#[test]
fn malformed_or_out_of_range_state_falls_back_to_default_position() {
    let directory = temporary_directory();
    let path = directory.join("placement");
    let store = PlacementStore::new(path.clone());

    for contents in ["garbage\n", "-1 20\n", "20 -1\n", "1 2 3\n"] {
        fs::write(&path, contents).expect("write malformed state");
        assert_eq!(
            store.load().expect("load malformed state"),
            Position::default()
        );
    }

    fs::remove_dir_all(directory).expect("remove temporary directory");
}

#[test]
fn state_path_prefers_xdg_state_home_and_falls_back_to_home() {
    assert_eq!(
        state_path(Some(OsStr::new("/state")), Some(OsStr::new("/home/me"))),
        Some(std::path::PathBuf::from("/state/wlapse/placement"))
    );
    assert_eq!(
        state_path(None, Some(OsStr::new("/home/me"))),
        Some(std::path::PathBuf::from(
            "/home/me/.local/state/wlapse/placement"
        ))
    );
    assert_eq!(state_path(None, None), None);
    assert_eq!(
        state_path(Some(OsStr::new("relative")), Some(OsStr::new("/home/me"))),
        Some(std::path::PathBuf::from(
            "/home/me/.local/state/wlapse/placement"
        ))
    );
}

#[test]
fn release_persists_the_latest_position() {
    let directory = temporary_directory();
    let path = directory.join("placement");
    let store = PlacementStore::new(path.clone());
    let mut placement = Placement::new(Position::default(), Some(store));

    placement.press();
    assert_eq!(placement.motion(8.0, 13.0), Some(Position { x: 8, y: 13 }));
    placement.release().expect("save on release");
    assert_eq!(
        PlacementStore::new(path)
            .load()
            .expect("load released position"),
        Position { x: 8, y: 13 }
    );

    fs::remove_dir_all(directory).expect("remove temporary directory");
}

#[test]
fn failed_release_can_retry_on_shutdown() {
    let directory = temporary_directory();
    let path = directory.join("placement");
    fs::create_dir(&path).expect("create conflicting directory");
    let store = PlacementStore::new(path.clone());
    let mut placement = Placement::new(Position::default(), Some(store));

    placement.press();
    assert_eq!(placement.motion(8.0, 13.0), Some(Position { x: 8, y: 13 }));
    assert!(placement.release().is_err());

    fs::remove_dir(&path).expect("remove conflicting directory");
    placement.shutdown().expect("retry save during shutdown");
    assert_eq!(
        PlacementStore::new(path).load().expect("load retried save"),
        Position { x: 8, y: 13 }
    );

    fs::remove_dir_all(directory).expect("remove temporary directory");
}

#[test]
fn shutdown_during_drag_persists_the_latest_position() {
    let directory = temporary_directory();
    let store = PlacementStore::new(directory.join("placement"));
    let mut placement = Placement::new(Position { x: 10, y: 20 }, Some(store));

    placement.press();
    assert_eq!(placement.motion(7.0, 9.0), Some(Position { x: 17, y: 29 }));
    placement.shutdown().expect("save during shutdown");

    let saved = PlacementStore::new(directory.join("placement"))
        .load()
        .expect("load saved placement");
    assert_eq!(saved, Position { x: 17, y: 29 });
    assert_eq!(placement.motion(1.0, 1.0), None);

    fs::remove_dir_all(directory).expect("remove temporary directory");
}
