use evdev_rs::enums::*;
use evdev_rs::TimeVal;
#[allow(unused)]
use log::{debug, error, info, trace, warn};
use serde::Deserialize;
use serde_repr::*;

/// The maximum travel before a tap is considered a swipe.
const MIN_SWIPE_DISTANCE: f64 = 300f64;
/// The maximum number of tools (fingers) that are initially tracked and reported on simultaneously.
const INITIAL_SLOTS: usize = 5;
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
            state: TouchpadState {
                slot_states: vec![None; INITIAL_SLOTS],
                ..Default::default()
            },
        }
    }

    pub fn add_event(
        &mut self,
        time: TimeVal,
        event_code: EventCode,
        event_value: i32,
    ) -> Option<Gesture> {
        match event_code {
            EventCode::EV_SYN(EV_SYN::SYN_REPORT) => {
                debug!("Processing report with {} events", self.report.events.len());
                let result = self.state.update(&mut self.report);
                self.report.events.clear();
                result
            }
            EventCode::EV_ABS(code) => {
                trace!("{code:?}: {event_value}");
                let time = time.tv_sec as f64 + time.tv_usec as f64 * 1E-6;
                self.report.events.push(SynEvent {
                    time,
                    evt_type: EventType::EV_ABS,
                    code: code as u16,
                    value: event_value,
                });
                None
            }
            EventCode::EV_KEY(code) => {
                trace!("{code:?}: {event_value}");
                let time = time.tv_sec as f64 + time.tv_usec as f64 * 1E-6;
                self.report.events.push(SynEvent {
                    time,
                    evt_type: EventType::EV_KEY,
                    code: code as u16,
                    value: event_value,
                });
                None
            }
            _ => {
                trace!("<IGNORED> {event_code:?}: {event_value}");
                None
            }
        }
    }
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
    code: u16,
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

#[derive(Clone, Debug, Default)]
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
    pub slot_states: Vec<Option<SlotState>>,
    pub start_xy: Option<Position>,
    pub end_xy: Option<Position>,
    pub last_ts: f64,
    pub last_gesture_time: f64,
    pub max_fingers: Option<Fingers>,
    pub last_finger: Option<Fingers>,
    pub gesture_start: Option<f64>,
    pub gesture_end: Option<f64>,
    pub with_btn_tool: bool,
    pub last_slot: Option<usize>,
}

#[derive(Clone, Debug, Default)]
struct SlotState {
    pub complete: bool,
    // pub tool_id: Option<i32>,
    // pub last_ts: f64,
    pub start_xy: Option<Position>,
    pub end_xy: Option<Position>,
}

impl SlotState {
    pub fn has_pos(&self) -> bool {
        self.start_xy.is_some()
    }

    /// Checks if a tool's `SlotState` indicates the tool is actually in use. Excludes a
    /// `SlotState` that has not yet been assigned a position (to filter out the default
    /// `SlotState` preemptively initialized to handle non-MT updates) and has not yet been marked
    /// as completed (to filter out fingers that have been removed from the touchpad).
    pub fn is_active(&self) -> bool {
        self.has_pos() && !self.complete
    }

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
        for slot in &mut self.slot_states {
            *slot = None;
        }
        self.start_xy = None;
        self.end_xy = None;
        // self.last_gesture_time should not be reset!
        // self.last_gesture_time = 0f64;
        self.max_fingers = None;
        self.last_finger = None;
        self.gesture_start = None;
        self.gesture_end = None;
    }

    fn update(&mut self, report: &mut SynReport) -> Option<Gesture> {
        let mut reset = false;
        let mut overall_x = None;
        let mut overall_y = None;

        // Loop over events and handle each slot separately
        {
            // A slot id is only specified if more than one tool is detected, so give `slot` a
            // default value in case we're not dealing with multi-touch gestures.
            let slot = &mut self.slot_states[self.last_slot.unwrap_or(0)];
            if slot.is_none() {
                *slot = Some(Default::default());
            }
            let mut slot = slot.as_mut().unwrap();
            // Some drivers don't generate a matching ABS_MT_POSITION_X/Y counterpart if the tool
            // hasn't moved on that axis (i.e. only moves in x or in y). Re-use old location in
            // that case.
            let mut slot_x = (slot.end_xy.as_ref())
                .or(slot.start_xy.as_ref())
                .map(|pos| pos.x);
            let mut slot_y = (slot.end_xy.as_ref())
                .or(slot.start_xy.as_ref())
                .map(|pos| pos.y);

            for event in &report.events {
                if event.time - self.last_ts >= EVENT_TIMEOUT {
                    reset = true;
                    break;
                }
                self.last_ts = event.time;

                match int_to_event_code(event.evt_type, event.code) {
                    EventCode::EV_ABS(EV_ABS::ABS_X) => {
                        // Overall location, regardless of tool
                        overall_x = Some(event.value);
                    }
                    EventCode::EV_ABS(EV_ABS::ABS_Y) => {
                        // Overall location, regardless of tool
                        overall_y = Some(event.value);
                    }
                    EventCode::EV_ABS(EV_ABS::ABS_MT_SLOT) => {
                        // This just tells us we're using a multitouch-capable trackpad and the
                        // id of the slot that contains information about the tool (finger) being
                        // tracked.
                        let slot_id = event.value as usize;
                        let s = match self.slot_states.get_mut(slot_id) {
                            Some(slot) => slot,
                            None => {
                                info!("Maximum slot count increased to {}", slot_id + 1);
                                self.slot_states.resize(slot_id + 1, None);
                                self.slot_states.get_mut(slot_id).unwrap()
                            }
                        };
                        *s = Some(Default::default());
                        slot = s.as_mut().unwrap();
                        self.last_slot = Some(slot_id);
                    }
                    EventCode::EV_ABS(EV_ABS::ABS_MT_POSITION_X) => {
                        slot_x = Some(event.value);
                        if slot_y.is_some() {
                            slot.push_position(slot_x.unwrap(), slot_y.unwrap());
                        }
                    }
                    EventCode::EV_ABS(EV_ABS::ABS_MT_POSITION_Y) => {
                        slot_y = Some(event.value);
                        if slot_x.is_some() {
                            slot.push_position(slot_x.unwrap(), slot_y.unwrap());
                        }
                    }

                    // Finger state applied
                    EventCode::EV_KEY(EV_KEY::BTN_TOOL_FINGER) if event.value == 1 => {
                        debug!("one finger press");
                        self.with_btn_tool = true;
                        self.gesture_start = Some(event.time);
                        self.last_finger.replace(Fingers::One);
                    }
                    EventCode::EV_KEY(EV_KEY::BTN_TOOL_DOUBLETAP) if event.value == 1 => {
                        debug!("two finger press");
                        self.with_btn_tool = true;
                        self.gesture_start = Some(event.time);
                        self.last_finger.replace(Fingers::Two);
                    }
                    EventCode::EV_KEY(EV_KEY::BTN_TOOL_TRIPLETAP) if event.value == 1 => {
                        debug!("three finger press");
                        self.with_btn_tool = true;
                        self.gesture_start = Some(event.time);
                        self.last_finger.replace(Fingers::Three);
                    }
                    EventCode::EV_KEY(EV_KEY::BTN_TOOL_QUADTAP) if event.value == 1 => {
                        debug!("four finger press");
                        self.with_btn_tool = true;
                        self.gesture_start = Some(event.time);
                        self.last_finger.replace(Fingers::Four);
                    }

                    // Physical button press registered ("force touch")
                    EventCode::EV_KEY(EV_KEY::BTN_LEFT | EV_KEY::BTN_RIGHT) => {
                        // If any gesture ended up pressing hard enough to trigger a physical click
                        // event, discard all the events in the report altogether.
                        debug!("disregarding gesture that included a physical button press");
                        reset = true;
                        break;
                    }

                    // Tracking of tool completed event
                    EventCode::EV_ABS(EV_ABS::ABS_MT_TRACKING_ID) if event.value == -1 => {
                        slot.complete = true;
                        self.last_slot = None;

                        let max_fingers = self.max_fingers.map(|m| m as u8).unwrap_or(0);
                        debug!("{} finger remove", max_fingers);

                        // We consider the gesture to be over when the first finger is removed.
                        // Everything after that is considered to be tear-down.

                        // self.with_btn_tool = true;
                        self.last_finger = None;
                        if self.gesture_end.is_none() {
                            self.gesture_end = Some(event.time);
                        }
                    }

                    // Catch-all
                    _ => {}
                };
            }
        }

        // Magic Mouse doesn't report BTN_TOOL_FINGER, BTN_TOOL_DOUBLETAP, etc. so we need a
        // fallback to track tool count.
        // See issue #9 and https://www.kernel.org/doc/Documentation/input/event-codes.txt
        if !report.events.is_empty() && !self.with_btn_tool {
            let active_tools = self
                .slot_states
                .iter()
                .filter(|s| s.as_ref().map(SlotState::is_active).unwrap_or(false))
                .count();
            let event_time = report.events.last().unwrap().time;
            let max_finger_count = self.max_fingers.map(|f| f as usize).unwrap_or(0);
            if active_tools > max_finger_count {
                debug!("{} finger press (calculated)", active_tools);
                self.gesture_start = Some(event_time);
                self.last_finger = Some(match active_tools {
                    1 => Fingers::One,
                    2 => Fingers::Two,
                    3 => Fingers::Three,
                    _ => Fingers::Four,
                });
            } else if active_tools < max_finger_count && self.last_finger.is_some() {
                debug!("{} finger remove (calculated)", max_finger_count);
                self.last_finger = None;
                self.gesture_end = Some(event_time);
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

        // Not all drivers produce ABS_X/ABS_Y to indicate the overall position. Apple Magic Mouse
        // seems to only produce the per-tool ABS_MT_POSITION_X/Y but not the overall ABS_X/Y.
        if overall_x.is_none() || overall_y.is_none() {
            // We can use the average of the positions as the overall location or we can just pick
            // a single tool and use that as a stand-in for our overall location.
            let pos = self.slot_states.iter().find_map(|state| match state {
                Some(state) => state.end_xy.as_ref().or(state.start_xy.as_ref()),
                None => None,
            });
            if let Some(pos) = pos {
                (overall_x, overall_y) = (Some(pos.x), Some(pos.y));
            }
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

        debug!("Remaining fingers: {:?}", self.last_finger);
        if self.last_finger.is_none() {
            if let Some(gesture) = self.process() {
                self.reset();
                return Some(gesture);
            }
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

        debug!("Distance: {distance}");

        trace!("self.last_ts: {}", self.last_ts);
        trace!("self.last_gesture_time: {}", self.last_gesture_time);
        if self.last_ts - self.last_gesture_time > DEBOUNCE_TIME {
            self.last_gesture_time = self.last_ts;
            if distance < MIN_SWIPE_DISTANCE {
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

fn int_to_event_code(ev_type: EventType, ev_code: u16) -> EventCode {
    // Since we converted the enum to a u16 in the first place, it is perfectly safe
    // to change it back as we know it'll be within the expected range of values.
    match ev_type {
        EventType::EV_ABS => EventCode::EV_ABS(unsafe { std::mem::transmute(ev_code as u8) }),
        EventType::EV_KEY => EventCode::EV_KEY(unsafe { std::mem::transmute(ev_code as u16) }),
        _ => unimplemented!(),
    }
}
