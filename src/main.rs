mod events;

use regex::Regex;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

fn main() {
    let mut event_loop = events::EventLoop::new();

    let evtest = Command::new("sudo")
        .args(&["evtest", "/dev/input/event11"])
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
                println!("{:?}", gesture);
            }
            continue;
        }

        if let Some(captures) = event_regex.captures(&line) {
            let time: f64 = captures[1].parse().unwrap();
            let event_type: u8 = captures[2].parse().unwrap();
            let code: u16 = captures[3].parse().unwrap();
            let value: i32 = captures[4].parse().unwrap();

            eprintln!("{}", line);
            event_loop.add_event(time, event_type, code, value);
        }
    }
}
