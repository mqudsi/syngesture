mod config;
mod events;

use config::Action;
use events::{EventLoop, Gesture};
use log::{info, trace};
use regex::Regex;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

fn main() {
    let config = config::load();

    if config.devices.is_empty() {
        eprintln!("No configured devices");
        std::process::exit(-1);
    }

    let mut threads = Vec::new();
    for (device, gestures) in config.devices {
        let handle = std::thread::spawn(move || {
            let mut event_loop = EventLoop::new();

            let evtest = Command::new("sudo")
                .args(&["evtest", &device])
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();

            // Event: time 1593656931.323635, type 3 (EV_ABS), code 47 (ABS_MT_SLOT), value 0
            let event_regex =
                Regex::new(r#"time (\d+\.\d+), type (\d+) .* code (\d+) .* value (\d+)"#).unwrap();

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

    if let Some(action) = gestures.get(&gesture) {
        match action {
            &Action::None => {},
            &Action::Execute(ref cmd) => {
                let mut shell = std::process::Command::new("sh");
                shell.args(&["-c", cmd]);
                if let Err(e) = shell.spawn() {
                    eprintln!("{}", e);
                }
            }
        }
    }
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
