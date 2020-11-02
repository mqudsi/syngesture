use log::{debug, trace};
use serde::Deserialize;
use serde_repr::*;

/// The maximum travel before a tap is considered a swipe, in millimeters.
const MAX_TAP_DISTANCE: f64 = 100f64;
/// The maximum number of tools (fingers) that are tracked and reported on simultaneously.
const MAX_SLOTS: usize = 5;
/// How long before the event state resets
const EVENT_TIMEOUT: f64 = 10_593_665_152f64;
/// A new gesture (note: not a new report) will not be entertained in this timespan.
const DEBOUNCE_TIME: f64 = 0.2f64;

pub(crate) struct EventLoop {
    report: SynReport,
    state: TouchpadState,
}

impl EventLoop {
    pub fn new() -> Self {
        Self {
            report: Default::default(),
            state: Default::default(),
        }
    }

    pub fn add_event(&mut self, time: f64, event_type: u8, event_code: u16, event_value: i32) {
        let event_type: EventType = match toml::Value::Integer(event_type as i64).try_into() {
            Ok(value) => value,
            Err(_) => {
                trace!("Unsupported event_type {}", event_type);
                return;
            }
        };
        let event_code: EventCode = match toml::Value::Integer(event_code as i64).try_into() {
            Ok(value) => value,
            Err(_) => {
                trace!("Unsupported event_code {}", event_code);
                return;
            }
        };

        self.report.events.push(SynEvent {
            time,
            evt_type: event_type,
            code: event_code,
            value: event_value,
        });
    }

    pub fn update(&mut self) -> Option<Gesture> {
        // eprintln!("Processing report with {} events", self.report.events.len());
        let result = self.state.update(&mut self.report);
        self.report.events.clear();
        return result;
    }
}

#[allow(non_camel_case_types, unused)]
#[derive(Deserialize_repr, Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
enum EventType {
    /// Unknown
    EV_SYN = 0,
    EV_KEY = 1,
    /// Absolute value pertaining to touchpad state (independent variable)
    EV_ABS = 3,
}

// Until it's proven that the different namespaces can collide (e.g. ABS_* and BTN_* sharing
// values), just keep them in one enum for our own sanity.
#[allow(non_camel_case_types, unused)]
#[derive(Deserialize_repr, Clone, Copy, Debug, PartialEq, PartialOrd)]
#[repr(u16)]
enum EventCode {
    // Absolute Events (reported per-tool)
    /// The overall x location, not differentiated by slot.
    ABS_X = 0,
    /// The overall y location, not differentiated by slot.
    ABS_Y = 1,
    /// The overall pressure, not differentiated by slot.
    ABS_PRESSURE = 24,
    /// The slot identifier
    ABS_MT_SLOT = 47,
    /// The per-tool x location
    ABS_MT_POSITION_X = 53,
    /// The per-tool y location
    ABS_MT_POSITION_Y = 54,
    /// The id of the tool being tracked in this slot
    ABS_MT_TRACKING_ID = 57,
    /// The per-tool pressure
    ABS_MT_PRESSURE = 58,

    // Key Events (reported globally)
    BTN_LEFT = 272,
    BTN_TOOL_FINGER = 325,
    BTN_TOUCH = 330,
    BTN_TOOL_DOUBLETAP = 333,
    BTN_TOOL_TRIPLETAP = 334,
    BTN_TOOL_QUADTAP = 335,
    BTN_TOOL_QUINTTAP = 328,
}

#[derive(Deserialize, Debug, PartialEq, PartialOrd, Eq, Ord)]
pub(crate) enum Direction {
    #[serde(alias = "up")]
    Up,
    #[serde(alias = "down")]
    Down,
    #[serde(alias = "left")]
    Left,
    #[serde(alias = "right")]
    Right,
}

#[repr(u8)]
#[derive(Deserialize_repr, Clone, Debug, PartialEq, PartialOrd, Copy, Eq, Ord)]
pub(crate) enum Fingers {
    One = 1,
    Two = 2,
    Three = 3,
    Four = 4,
}

// Used to abstract away the event source. In the future, we can migrate from
// using evtest to reading from the input device directly.
#[derive(Debug, PartialEq)]
struct SynEvent {
    time: f64,
    evt_type: EventType,
    code: EventCode,
    value: i32,
}

/// A grouping of [`SynEvent`] objects that arrive together in one report.
/// Each individual `SynEvent` still has its own timestamp.
#[derive(Debug, Default)]
struct SynReport {
    events: Vec<SynEvent>,
}

/// A result derived from one or more [`SynReport`] instances in a stream.
#[derive(Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
#[derive(Debug, PartialEq, PartialOrd, Eq, Ord)]
pub(crate) enum Gesture {
    Tap {
        fingers: Fingers,
    },
    Swipe {
        fingers: Fingers,
        direction: Direction,
    },
}

#[derive(Debug, Default)]
struct Position {
    x: i32,
    y: i32,
}

fn pos(x: i32, y: i32) -> Position {
    Position { x, y }
}

/// Returns the Euclidean distance between two positions
fn get_distance(pos1: &Position, pos2: &Position) -> f64 {
    (((pos2.x - pos1.x).pow(2) + (pos2.y - pos1.y).pow(2)) as f64).sqrt()
}

fn get_direction(pos1: &Position, pos2: &Position) -> Direction {
    // It's much easier to scroll side-to-side than up-down, so include a bias
    if (pos2.x - pos1.x).abs() > ((1.05f64 * (pos2.y - pos1.y) as f64) as i32).abs() {
        // Interpret as movement along the x-axis only
        if pos2.x > pos1.x {
            Direction::Right
        } else {
            Direction::Left
        }
    } else {
        // Interpret as movement along the y-axis only
        if pos2.y > pos1.y {
            Direction::Down
        } else {
            Direction::Up
        }
    }
}

/// A multitouch trackpad driver tracks the location of each tool (read: finger) in a separate
/// slot, and reports on all of them simultaneously. Each tool is independently tracked and does
/// not affect the state of any other tool/slot.
///
/// `TouchpadState` tracks the status of all slots.
#[derive(Debug, Default)]
struct TouchpadState {
    pub slot_states: [Option<SlotState>; MAX_SLOTS],
    pub start_xy: Option<Position>,
    pub end_xy: Option<Position>,
    pub last_ts: f64,
    pub last_gesture_time: f64,
    pub max_fingers: Option<Fingers>,
    pub last_finger: Option<Fingers>,
    pub finger_start: Option<f64>,
    pub one_finger_duration: f64,
    pub two_finger_duration: f64,
    pub three_finger_duration: f64,
    pub four_finger_duration: f64,
}

#[derive(Debug, Default)]
struct SlotState {
    pub complete: bool,
    pub tool_id: Option<i32>,
    pub last_ts: f64,
    pub start_xy: Option<Position>,
    pub end_xy: Option<Position>,
}

impl SlotState {
    pub fn push_position(&mut self, x: i32, y: i32) {
        if self.start_xy.is_none() {
            self.start_xy = Some(pos(x, y));
        } else {
            self.end_xy = Some(pos(x, y));
        }
    }

    #[allow(unused)]
    pub fn distance(&self) -> Option<f64> {
        if let (Some(start_xy), Some(end_xy)) = (&self.start_xy, &self.end_xy) {
            Some(get_distance(start_xy, end_xy))
        } else {
            None
        }
    }

    #[allow(unused)]
    pub fn direction(&self) -> Option<Direction> {
        if let (Some(start_xy), Some(end_xy)) = (&self.start_xy, &self.end_xy) {
            Some(get_direction(start_xy, end_xy))
        } else {
            None
        }
    }
}

impl TouchpadState {
    pub fn reset(&mut self) {
        debug!("***RESET***");
        self.slot_states = Default::default();
        self.start_xy = None;
        self.end_xy = None;
        // self.last_gesture_time should not be reset!
        // self.last_gesture_time = 0f64;
        self.max_fingers = None;
        self.last_finger = None;
        self.finger_start = None;
        self.one_finger_duration = 0f64;
        self.two_finger_duration = 0f64;
        self.three_finger_duration = 0f64;
        self.four_finger_duration = 0f64;
    }

    pub fn update(&mut self, report: &mut SynReport) -> Option<Gesture> {
        let mut reset = false;
        let mut overall_x = None;
        let mut overall_y = None;

        // Loop over events and handle each slot separately
        {
            let prev_finger_start = self.finger_start;
            let mut slot = &mut self.slot_states[0];
            #[allow(unused_assignments)]
            let mut slot_id = 0usize;
            // A slot id is only specified if more than one tool is detected.
            if slot.is_none() {
                *slot = Some(Default::default());
            }
            let mut slot_x = None;
            let mut slot_y = None;
            for event in &report.events {
                if event.time - self.last_ts >= EVENT_TIMEOUT {
                    reset = true;
                    break;
                }
                self.last_ts = event.time;

                match (&event.evt_type, &event.code) {
                    (EventType::EV_ABS, EventCode::ABS_X) => {
                        // Overall location, regardless of tool
                        overall_x = Some(event.value);
                    }
                    (EventType::EV_ABS, EventCode::ABS_Y) => {
                        // Overall location, regardless of tool
                        overall_y = Some(event.value);
                    }
                    (EventType::EV_ABS, EventCode::ABS_MT_SLOT) => {
                        // This just tells us we're using a multitouch-capable trackpad and the
                        // id of the slot that contains information about the tool (finger) being
                        // tracked.
                        slot_id = event.value as usize;
                        self.slot_states[slot_id] = Some(Default::default());
                        slot = &mut self.slot_states[slot_id];
                    }
                    (EventType::EV_ABS, EventCode::ABS_MT_POSITION_X) => {
                        slot_x = Some(event.value);
                        if slot_y.is_some() {
                            slot.as_mut()
                                .unwrap()
                                .push_position(slot_x.take().unwrap(), slot_y.take().unwrap());
                        }
                    }
                    (EventType::EV_ABS, EventCode::ABS_MT_POSITION_Y) => {
                        slot_y = Some(event.value);
                        if slot_x.is_some() {
                            slot.as_mut()
                                .unwrap()
                                .push_position(slot_x.take().unwrap(), slot_y.take().unwrap());
                        }
                    }

                    // Finger state applied
                    (EventType::EV_KEY, EventCode::BTN_TOOL_FINGER) if event.value == 1 => {
                        debug!("one finger press");
                        self.finger_start = Some(event.time);
                        self.last_finger.replace(Fingers::One);
                    }
                    (EventType::EV_KEY, EventCode::BTN_TOOL_DOUBLETAP) if event.value == 1 => {
                        debug!("two finger press");
                        self.finger_start = Some(event.time);
                        self.last_finger.replace(Fingers::Two);
                    }
                    (EventType::EV_KEY, EventCode::BTN_TOOL_TRIPLETAP) if event.value == 1 => {
                        debug!("three finger press");
                        self.finger_start = Some(event.time);
                        self.last_finger.replace(Fingers::Three);
                    }
                    (EventType::EV_KEY, EventCode::BTN_TOOL_QUADTAP) if event.value == 1 => {
                        debug!("four finger press");
                        self.finger_start = Some(event.time);
                        self.last_finger.replace(Fingers::Four);
                    }

                    // Finger state removed
                    // Assuming we never miss an event, the finger should always have started
                    (EventType::EV_KEY, EventCode::BTN_TOOL_FINGER) if event.value == 0 => {
                        if prev_finger_start.is_some() {
                            debug!(
                                "one finger remove {}",
                                event.time - prev_finger_start.unwrap()
                            );
                            self.one_finger_duration += event.time - prev_finger_start.unwrap();
                        }
                        self.last_finger = None;
                    }
                    (EventType::EV_KEY, EventCode::BTN_TOOL_DOUBLETAP) if event.value == 0 => {
                        if prev_finger_start.is_some() {
                            debug!(
                                "two finger remove {}",
                                event.time - prev_finger_start.unwrap()
                            );
                            self.two_finger_duration += event.time - prev_finger_start.unwrap();
                        }
                        self.last_finger = None;
                    }
                    (EventType::EV_KEY, EventCode::BTN_TOOL_TRIPLETAP) if event.value == 0 => {
                        if prev_finger_start.is_some() {
                            debug!(
                                "three finger remove {}",
                                event.time - prev_finger_start.unwrap()
                            );
                            self.three_finger_duration += event.time - prev_finger_start.unwrap();
                        }
                        self.last_finger = None;
                    }
                    (EventType::EV_KEY, EventCode::BTN_TOOL_QUADTAP) if event.value == 0 => {
                        if prev_finger_start.is_some() {
                            debug!(
                                "four finger remove {}",
                                event.time - prev_finger_start.unwrap()
                            );
                            self.four_finger_duration += event.time - prev_finger_start.unwrap();
                        }
                        self.last_finger = None;
                    }

                    // Tracking complete event
                    (EventType::EV_ABS, EventCode::ABS_MT_TRACKING_ID) if event.value == -1 => {
                        slot.as_mut().unwrap().complete = true;
                    }

                    // Catch-all
                    _ => {}
                };
            }
        }

        if reset {
            self.reset();
            return None;
        }

        if self.max_fingers.is_none() || self.last_finger > self.max_fingers {
            // Reset start position because everything until now was presumably building to this
            self.start_xy = None;
            self.max_fingers = self.last_finger;
        }

        if let (Some(x), Some(y)) = (overall_x.take(), overall_y.take()) {
            // We always consider a decrease in tool count to be a tear-down and ignore the change
            // in position.
            if self.max_fingers == self.last_finger {
                self.push_position(x, y);
            } else {
                debug!("Position ignored");
            }
        }

        if self.last_finger.is_none() {
            if let Some(gesture) = self.process() {
                self.reset();
                return Some(gesture);
            }
        } else {
            debug!("Remaining finger(s): {:?}", self.last_finger);
        }

        return None;
    }

    pub fn push_position(&mut self, x: i32, y: i32) {
        if self.start_xy.is_none() {
            self.start_xy = Some(pos(x, y));
        } else {
            self.end_xy = Some(pos(x, y));
        }
    }

    fn process(&mut self) -> Option<Gesture> {
        if self.start_xy.is_none() {
            debug!("Received report but indeterminate start");
            return None;
        }

        // What if we always assume that the maximum number of fingers detected
        // was the intended click?
        let fingers = match self.max_fingers {
            Some(finger) => finger,
            None => {
                debug!("Received report without any tools detected");
                return None;
            }
        };

        let distance = match &self.end_xy {
            Some(end_xy) => get_distance(self.start_xy.as_ref().unwrap(), &end_xy),
            None => 0f64,
        };

        debug!("Distance: {}", distance);

        trace!("self.last_ts: {}", self.last_ts);
        trace!("self.last_gesture_time: {}", self.last_gesture_time);
        if self.last_ts - self.last_gesture_time > DEBOUNCE_TIME {
            self.last_gesture_time = self.last_ts;
            if distance < MAX_TAP_DISTANCE {
                debug!("tap detected");
                Some(Gesture::Tap { fingers })
            } else {
                debug!("gesture detected");
                Some(Gesture::Swipe {
                    fingers,
                    direction: get_direction(
                        self.start_xy.as_ref().unwrap(),
                        self.end_xy.as_ref().unwrap(),
                    ),
                })
            }
        } else {
            debug!("Gesture ignored by debounce");
            None
        }
    }
}
