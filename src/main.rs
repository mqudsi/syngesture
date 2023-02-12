mod config;
mod events;

use config::Action;
use evdev_rs::Device as EvDevice;
use events::{EventLoop, Gesture};
use log::{info, trace, warn};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::str;

fn print_version<W: std::io::Write>(target: &mut W) {
    let _ = writeln!(
        target,
        "syngestures {} - Copyright NeoSmart Technologies 2020-2022",
        env!("CARGO_PKG_VERSION")
    );

    for line in [
        "Developed by Mahmoud Al-Qudsi and other syngestures contributors",
        "Report bugs at <https://github.com/mqudsi/syngesture>",
    ] {
        writeln!(target, "{line}").ok();
    }
}

fn print_help<W: std::io::Write>(target: &mut W) {
    print_version(&mut *target);
    for line in [
        "",
        "Usage: syngestures [OPTIONS]",
        "",
        "Options:",
        "  -h --help     Print this help message",
        "  -V --version  Print version info",
        "",
        "A valid syngestures config file must be installed to one of the",
        "following locations before executing syngestures:",
    ] {
        writeln!(target, "{}", line).ok();
    }

    for dir in config::config_dirs() {
        writeln!(target, "  * {}", dir).ok();
    }

    for line in [
        "",
        "A sample configuration file can be found in the package tarball or online at",
    ] {
        writeln!(target, "{}", line).ok();
    }

    let _ = writeln!(
        target,
        "<https://raw.githubusercontent.com/mqudsi/syngesture/{}/syngestures.toml>",
        env!("CARGO_PKG_VERSION")
    );
}

#[cfg(feature = "debug")]
fn init_logger() {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "trace");
    }
    pretty_env_logger::init();
}

fn main() {
    #[cfg(feature = "debug")]
    init_logger();

    let args = std::env::args();
    for arg in args.skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help(&mut std::io::stdout());
                std::process::exit(0);
            }
            "-V" | "--version" => {
                print_version(&mut std::io::stdout());
                std::process::exit(0);
            }
            _ => {
                eprintln!("{}: Invalid option!", arg);
                eprintln!("Try 'syngestures --help' for more info");
                std::process::exit(-1);
            }
        }
    }

    let config = config::load();

    if config.devices.is_empty() {
        eprintln!("No configured devices");
        std::process::exit(-1);
    }

    let mut threads = Vec::new();
    for (device_path, gestures) in config.devices {
        let device = match EvDevice::new_from_path(&device_path) {
            Ok(device) => device,
            Err(e) => {
                eprintln!("{}: {}", device_path, e);
                continue;
            }
        };
        let handle = std::thread::spawn(move || {
            use evdev_rs::{InputEvent, ReadFlag, ReadStatus};
            use evdev_rs::enums::*;

            let mut event_loop = EventLoop::new();
            let mut read_flag = ReadFlag::NORMAL;
            loop {
                let event = match device.next_event(read_flag) {
                    Ok((ReadStatus::Success, event)) => event,
                    Ok((ReadStatus::Sync, InputEvent { event_code: EventCode::EV_SYN(EV_SYN::SYN_DROPPED), .. })) => {
                        read_flag = ReadFlag::SYNC;
                        continue;
                    }
                    Ok((ReadStatus::Sync, event)) => event,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                    Err(e) => {
                        eprintln!("{}: {}", device_path, e);
                        break;
                    }
                };

                if let Some(gesture) = event_loop.add_event(event.time, event.event_code, event.value) {
                    swipe_handler(&gestures, gesture);
                }
            }
        });
        threads.push(handle);
    }

    for thread in threads {
        thread.join().unwrap();
    }
}

fn swipe_handler(gestures: &config::GestureMap, gesture: Gesture) {
    info!("{:?}", gesture);

    let action = match gestures.get(&gesture) {
        Some(action) => action,
        None => return,
    };

    match action {
        &Action::None => {}
        &Action::Execute(ref cmd) => {
            let mut shell = Command::new("sh");
            shell.args(&["-c", cmd]);
            let mut child = match shell.spawn() {
                Ok(child) => child,
                Err(e) => {
                    eprintln!("{}", e);
                    return;
                }
            };

            // Spawn a thread to wait on the process to finish executing.
            // This is only here to avoid zombie processes from piling up.
            // TODO: Just have one thread wait on all launched processes.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

fn which(target: &str) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::unix::prelude::OsStringExt;

    let mut cmd = Command::new("which");
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());
    cmd.args(&[target]);
    let output = match cmd.output() {
        Err(_) => {
            warn!("Failed to find/execute `which`");
            return None;
        }
        Ok(output) => output,
    };

    if output.status.success() {
        let path = OsString::from_vec(output.stdout);
        return Some(PathBuf::from(path));
    }

    return None;
}
