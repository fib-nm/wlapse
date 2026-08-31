use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use wlapse::config::{config_path, load};
use wlapse::instance::{AcquireResult, Instance};
use wlapse::placement::{Placement, PlacementStore, state_path};
use wlapse::wayland::WaylandApp;

fn main() {
    let mut arguments = std::env::args_os();
    let _program = arguments.next();
    match (arguments.next(), arguments.next()) {
        (None, None) => {}
        (Some(argument), None) if argument == "--help" => {
            print!(
                "wlapse - A lightweight, on-demand stopwatch overlay for Wayland\n\n\
                 Usage:\n  wlapse\n  wlapse --help\n  wlapse --version\n\n\
                 Run without arguments to show or stop the stopwatch.\n"
            );
            return;
        }
        (Some(argument), None) if argument == "--version" => {
            println!("wlapse {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        _ => {
            eprintln!("wlapse: this command does not accept arguments");
            std::process::exit(2);
        }
    }

    if let Err(error) = run() {
        eprintln!("wlapse: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "XDG_RUNTIME_DIR is not set".to_owned())?;

    let instance = match Instance::acquire(&runtime_dir)
        .map_err(|error| format!("cannot acquire instance socket: {error}"))?
    {
        AcquireResult::Owner(instance) => instance,
        AcquireResult::StoppedExisting => return Ok(()),
    };

    let config_path = config_path(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    );
    let colors =
        load(config_path.as_deref()).map_err(|error| format!("cannot load config: {error}"))?;

    let terminate = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, terminate.clone())
        .map_err(|error| format!("cannot register SIGINT handler: {error}"))?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, terminate.clone())
        .map_err(|error| format!("cannot register SIGTERM handler: {error}"))?;

    let store = PlacementStore::new(
        state_path(
            std::env::var_os("XDG_STATE_HOME").as_deref(),
            std::env::var_os("HOME").as_deref(),
        )
        .ok_or_else(|| {
            "XDG_STATE_HOME and HOME do not provide an absolute state directory".to_owned()
        })?,
    );
    let position = store
        .load()
        .map_err(|error| format!("cannot load saved placement: {error}"))?;
    let placement = Placement::new(position, Some(store));

    WaylandApp::connect(placement, colors)
        .map_err(|error| error.to_string())?
        .run(&instance, terminate)
        .map_err(|error| error.to_string())
}
