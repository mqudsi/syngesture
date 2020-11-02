mod config;
mod events;

use config::Action;
use events::{EventLoop, Gesture};
use log::{info, trace, warn};
use regex::Regex;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

fn main() {
    let config = config::load();

    if which("evtest").is_none() {
        eprintln!("Cannot find `evtest` - make sure it is installed and try again!");
        std::process::exit(-1);
    }

    if config.devices.is_empty() {
        eprintln!("No configured devices");
        std::process::exit(-1);
    }

    // Event: time 1593656931.323635, type 3 (EV_ABS), code 47 (ABS_MT_SLOT), value 0
    let event_regex = std::sync::Arc::new(
        Regex::new(r#"time (\d+\.\d+), type (\d+) .* code (\d+) .* value (\d+)"#).unwrap(),
    );

    let mut threads = Vec::new();
    for (device, gestures) in config.devices {
        let event_regex = event_regex.clone();
        let handle = std::thread::spawn(move || {
            let mut event_loop = EventLoop::new();

            let evtest = Command::new("evtest")
                .args(&[&device])
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();

            let reader = BufReader::new(evtest.stdout.unwrap());
            for line in reader.lines() {
                let line = match line {
                    Ok(line) => line,
                    Err(_) => break,
                };

                // Event: time 1593656931.306879, -------------- SYN_REPORT ------------
                if line.contains("SYN_REPORT") {
                    if let Some(gesture) = event_loop.update() {
                        swipe_handler(&gestures, gesture);
                    }
                    continue;
                }

                if let Some(captures) = event_regex.captures(&line) {
                    let time: f64 = captures[1].parse().unwrap();
                    let event_type: u8 = captures[2].parse().unwrap();
                    let code: u16 = captures[3].parse().unwrap();
                    let value: i32 = captures[4].parse().unwrap();

                    trace!("{}", line);
                    event_loop.add_event(time, event_type, code, value);
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
            // TODO: Just have one thread wait on all launched processes.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

fn which(target: &str) -> Option<String> {
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
        let result = match String::from_utf8(output.stdout) {
            Ok(result) => result,
            Err(_) => {
                warn!("Path to {} cannot be converted to a UTF-8 string!", target);
                return None;
            }
        };
        return Some(result);
    }

    return None;
}

// fn xdotool(command: &'static str, actions: &'static str) {
//     use std::thread;
//
//     thread::spawn(move || {
//         Command::new("xdotool")
//             .args(&[command, actions])
//             .output()
//             .expect("Failed to run xdotool");
//     });
// }
