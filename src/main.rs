mod config;
mod epoll;
#[cfg(not(feature = "logging"))]
mod errorlog;
mod events;

use config::Action;
use epoll::Epoll;
use evdev_rs::Device as EvDevice;
use events::{EventLoop, Gesture};
#[allow(unused)]
use log::{debug, error, info, trace, warn};
use std::io::ErrorKind;
use std::os::fd::AsRawFd;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

static SIGHUP: AtomicBool = AtomicBool::new(false);

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
        writeln!(target, "{line}").ok();
    }

    for dir in config::config_dirs() {
        writeln!(target, "  * {dir}").ok();
    }

    for line in [
        "",
        "A sample configuration file can be found in the package tarball or online at",
    ] {
        writeln!(target, "{line}").ok();
    }

    let _ = writeln!(
        target,
        "<https://raw.githubusercontent.com/mqudsi/syngesture/{}/syngestures.toml>",
        env!("CARGO_PKG_VERSION")
    );
}

#[cfg(feature = "logging")]
fn init_logger() {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "trace");
    }
    pretty_env_logger::init();
}

#[cfg(not(feature = "logging"))]
fn init_logger() {
    errorlog::init();
}

extern "C" fn on_sighup(_: libc::c_int) {
    SIGHUP.store(true, Ordering::Relaxed);
}

fn main() {
    init_logger();

    let args = std::env::args();
    #[allow(clippy::never_loop)]
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
                eprintln!("{arg}: Invalid option!");
                eprintln!("Try 'syngestures --help' for more info");
                std::process::exit(-1);
            }
        }
    }

    // Install a SIGHUP handler to tell us to reload the configuration file
    unsafe {
        let result = libc::signal(libc::SIGHUP, on_sighup as libc::sighandler_t);
        assert_eq!(result, 0);
    }

    // Tell the kernel to reap child processes automatically and not require a wait(2) call.
    // Note that this probably completely breaks waiting on child processes to complete!
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_flags = libc::SA_NOCLDWAIT;
        let result = libc::sigaction(libc::SIGCHLD, &sa as *const _, std::ptr::null_mut());
        assert_eq!(result, 0);
    }

    loop {
        let config = config::load();
        if config.devices.is_empty() {
            error!("No configured devices");
            std::process::exit(-1);
        }

        std::thread::scope(|scope| {
            watch_devices(scope, config);

            // We hang here until all device watcher threads have terminated.
            // That's OK for now, but in case of SIGHUP the worker threads won't notice the signal
            // until they wake after receiving an epoll(7) event.
            // TODO: Give the threads a binary semaphore to add their epoll queue and signal it in
            // our own SIGHUP handler so they wake immediately.
        });

        if SIGHUP.swap(false, Ordering::Relaxed) {
            info!("Reloading after SIGHUP");
            continue;
        }
        break;
    }
}

fn watch_devices<'scope>(
    scope: &'scope std::thread::Scope<'scope, '_>,
    config: config::Configuration,
) {
    for (device_path, gestures) in config.devices {
        let device = match EvDevice::new_from_path(&device_path) {
            Ok(device) => device,
            Err(e) => {
                error!("{device_path}: {e}");
                continue;
            }
        };
        let device_fd = device.file().as_raw_fd();
        scope.spawn(move || {
            use evdev_rs::enums::*;
            use evdev_rs::{InputEvent, ReadFlag, ReadStatus};

            let mut epoll = Epoll::new().unwrap();
            epoll.register_read(device_fd, false).unwrap();

            let mut event_loop = EventLoop::new();
            let mut read_flag = ReadFlag::NORMAL;
            'device: loop {
                if SIGHUP.load(Ordering::Relaxed) {
                    debug!("Threading exiting because SIGHUP was set.");
                    return;
                }
                let event = match device.next_event(read_flag) {
                    Ok((ReadStatus::Success, event)) => event,
                    Ok((
                        ReadStatus::Sync,
                        InputEvent {
                            event_code: EventCode::EV_SYN(EV_SYN::SYN_DROPPED),
                            ..
                        },
                    )) => {
                        read_flag = ReadFlag::SYNC;
                        continue;
                    }
                    Ok((ReadStatus::Sync, event)) => event,
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        read_flag = ReadFlag::NORMAL;
                        loop {
                            match epoll.wait(None) {
                                Ok(()) => continue 'device,
                                Err(e) => {
                                    if e.kind() == ErrorKind::Interrupted {
                                        continue;
                                    }
                                    error!("epoll_wait: {e}");
                                    break 'device;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("{device_path}: {e}");
                        break;
                    }
                };

                let result = event_loop.add_event(event.time, event.event_code, event.value);
                if let Some(gesture) = result {
                    swipe_handler(&gestures, gesture);
                }
            }
        });
    }
}

fn swipe_handler(gestures: &config::GestureMap, gesture: Gesture) {
    info!("{:?}", gesture);

    let action = match gestures.get(&gesture) {
        Some(action) => action,
        None => return,
    };

    match action {
        Action::None => {}
        Action::Execute(cmd) => {
            let mut shell = Command::new("sh");
            shell.args(["-c", cmd]);
            // We have SA_NOCLDWAIT set up, so there's no need to wait for children to prevent
            // zombies.
            if let Err(err) = shell.spawn() {
                error!("{err}");
                return;
            };
        }
    }
}
