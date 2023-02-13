use crate::events::*;
#[allow(unused)]
use log::{debug, error, info, trace, warn};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

const PREFIX: Option<&'static str> = option_env!("PREFIX");

pub(crate) type Device = String;
pub(crate) type GestureMap = BTreeMap<Gesture, Action>;

type BoxedError = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, BoxedError>;

pub(crate) struct Configuration {
    pub devices: BTreeMap<Device, GestureMap>,
}

impl Configuration {
    pub fn new() -> Self {
        Self {
            devices: Default::default(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Action {
    #[serde(skip)]
    None,
    Execute(String),
}

impl Default for Action {
    fn default() -> Self {
        Action::None
    }
}

fn get_prefix() -> PathBuf {
    PathBuf::from(PREFIX.unwrap_or("/usr/local"))
}

pub(crate) fn config_dirs() -> Vec<String> {
    let prefix = get_prefix();

    vec![
        format!("{}/etc/syngestures.toml", prefix.display()),
        format!("{}/etc/syngestures.d/*.toml", prefix.display()),
        "$XDG_HOME/syngestures.toml".to_owned(),
        "$XDG_HOME/syngestures.d/*.toml".to_owned(),
        "$HOME/.config/syngestures.toml".to_owned(),
        "$HOME/.config/syngestures.d/*.toml".to_owned(),
    ]
}

pub(crate) fn load() -> Configuration {
    let mut config = Configuration::new();

    let prefix = get_prefix();
    let global_config = prefix.join("etc/syngestures.toml");

    if global_config.exists() {
        try_load_config_file(&mut config, &global_config);
    }

    let global_config_dir = prefix.join("etc/syngestures.d");
    try_load_config_dir(&mut config, &global_config_dir);

    load_user_config(&mut config);

    if config.devices.is_empty() {
        error!("No configuration found!");
        eprintln!("Searched for configuration files in the following locations:");
        for dir in config_dirs() {
            eprintln!("* {dir}");
        }
    }

    config
}

fn try_load_config_file(config: &mut Configuration, path: &Path) {
    if let Err(e) = load_config_file(config, &path) {
        error!(
            "Error loading configuration file at {}: {}",
            path.display(),
            e
        );
    }
}

fn try_load_config_dir(config: &mut Configuration, dir: &Path) {
    if let Err(e) = load_config_dir(config, &dir) {
        error!(
            "Error reading from configuration directory {}: {}",
            dir.display(),
            e
        );
    }
}

fn load_user_config(mut config: &mut Configuration) {
    let config_home = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(xdg_config_home) => PathBuf::from(xdg_config_home),
        None => match get_user_config_dir() {
            Ok(dir) => dir,
            Err(e) => {
                error!("{e}");
                return;
            }
        },
    };

    let user_config_file = config_home.join("syngestures.toml");
    if user_config_file.exists() {
        try_load_config_file(&mut config, &user_config_file);
    }

    let user_config_dir = config_home.join("syngestures.d");
    try_load_config_dir(&mut config, &user_config_dir);
}

fn get_user_config_dir() -> Result<PathBuf> {
    #[allow(deprecated)]
    let home = std::env::home_dir();

    if home.is_none() || home.as_ref().unwrap() == &PathBuf::new() {
        return Err("Could not determine user home directory!".into());
    }

    let config_home = home.unwrap().join(".config/");
    Ok(config_home)
}

fn load_config_dir(mut config: &mut Configuration, dir: &Path) -> Result<()> {
    use std::fs::DirEntry;

    if !dir.exists() || !dir.is_dir() {
        return Ok(());
    }

    let toml = OsStr::new("toml");
    for item in dir.read_dir()? {
        let item = match item {
            Ok(item) => item,
            Err(e) => {
                error!(
                    "Error reading file from configuration directory {}: {e}",
                    dir.display()
                );
                continue;
            }
        };

        // in lieu of try_block...
        let mut process_item = |item: &DirEntry| -> Result<()> {
            if item.file_type()?.is_dir() {
                return Ok(());
            }

            let item = item.path();
            if item.extension() != Some(toml) {
                return Ok(());
            }

            try_load_config_file(&mut config, &item);
            Ok(())
        };

        if let Err(e) = process_item(&item) {
            error!("Error loading {}: {e}", item.path().display());
        }
    }

    Ok(())
}

fn load_config_file(config: &mut Configuration, path: &Path) -> Result<()> {
    #[derive(Deserialize)]
    struct ConfigGestureAndAction {
        #[serde(flatten)]
        pub gesture: Gesture,
        #[serde(flatten)]
        pub action: Action,
    }

    #[derive(Deserialize)]
    struct ConfigDeviceGestures {
        pub device: Device,
        pub gestures: Vec<ConfigGestureAndAction>,
    }

    #[derive(Deserialize)]
    struct ConfigFile {
        #[serde(alias = "device")]
        pub devices: Vec<ConfigDeviceGestures>,
    }

    let bytes = std::fs::read(path)?;
    let config_file: ConfigFile = toml::from_slice(&bytes)?;

    for device_config in config_file.devices {
        let device = device_config.device;

        let device_gestures = config.devices.entry(device).or_default();
        for gesture_action in device_config.gestures {
            device_gestures.insert(gesture_action.gesture, gesture_action.action);
        }
    }

    Ok(())
}
