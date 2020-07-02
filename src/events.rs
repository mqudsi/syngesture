/// The maximum travel before a tap is considered a swipe, in millimeters.
const MAX_TAP_DISTANCE: f32 = 2.0f32;

/// The maximum number of tools (fingers) that are tracked and reported on simultaneously.
const MAX_SLOTS: usize = 5;

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug)]
enum EventType {
    /// Unknown
    EV_SYN = 0,
    EV_KEY = 1,
    /// Absolute value pertaining to touchpad state (independent variable)
    EV_ABS = 3,
}

// Until it's proven that the different namespaces can collide (e.g. ABS_* and BTN_* sharing
// values), just keep them in one enum for our own sanity.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug)]
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
    BTN_TOOL_QUINTTAP = 328,
    BTN_TOUCH = 330,
    BTN_TOOL_DOUBLETAP = 333,
    BTN_TOOL_TRIPLETAP = 334,
    BTN_TOOL_QUADTAP = 335,
}

enum Direction {
    None,
    Up,
    Down,
    Left,
    Right,
}

enum Fingers {
    One = 1,
    Two = 2,
    Three = 3,
    Four = 4,
}

// Used to abstract away the event source. In the future, we can migrate from
// using evtest to reading from the input device directly.
struct SynEvent {
    time: f32,
    evt_type: EventType,
    code: EventCode,
    value: i32,
}

/// A grouping of [`SynEvent`] objects that arrive together in one report.
/// Each individual `SynEvent` still has its own timestamp.
struct SynReport {
    events: Vec<SynEvent>,
}

/// A result derived from one or more [`SynReport`] instances in a stream.
pub(crate) enum Gesture {
    Tap(Fingers),
    Swipe(Fingers, Direction),
}

/// How long before the event state resets
const EVENT_TIMEOUT: f32 = 2f32;

#[derive(Debug, Default)]
struct Position {
    x: i32,
    y: i32,
}

fn pos(x: i32, y: i32) -> Position {
    Position { x, y }
}

/// Returns the Euclidean distance between two positions
fn get_distance(pos1: &Position, pos2: &Position) -> f32 {
    (((pos2.x - pos1.x).pow(2) + (pos2.y - pos1.y).pow(2)) as f32).sqrt()
}

fn get_direction(pos1: &Position, pos2: &Position) -> Direction {
    if (pos2.x - pos1.x).abs() > (pos2.y - pos1.y).abs() {
        // Interpret as movement along the x-axis only
        if pos2.x > pos1.x {
            Direction::Right
        } else {
            Direction::Left
        }
    } else {
        // Interpret as movement along the y-axis only
        if pos2.y > pos1.y {
            Direction::Up
        } else {
            Direction::Down
        }
    }
}

/// A multitouch trackpad driver tracks the location of each tool (read: finger) in a separate
/// slot, and reports on all of them simultaneously. Each tool is independently tracked and does
/// not affect the state of any other tool/slot.
///
/// `TouchpadState` tracks the status of all slots.
struct TouchpadState {
    pub slot_states: [Option<SlotState>; MAX_SLOTS],
    pub start_xy: Option<Position>,
    pub end_xy: Option<Position>,
    pub last_ts: f32,
    pub last_finger: Option<Fingers>,
    pub finger_start: Option<f32>,
    pub one_finger_duration: f32,
    pub two_finger_duration: f32,
    pub three_finger_duration: f32,
    pub four_finger_duration: f32,
}

#[derive(Debug, Default)]
struct SlotState {
    pub complete: bool,
    pub tool_id: Option<i32>,
    pub last_ts: f32,
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

    pub fn distance(&self) -> Option<f32> {
        if let (Some(start_xy), Some(end_xy)) = (&self.start_xy, &self.end_xy) {
            Some(get_distance(start_xy, end_xy))
        } else {
            None
        }
    }

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
        self.slot_states = Default::default();
        self.start_xy = None;
        self.end_xy = None;
        self.last_finger = None;
        self.finger_start = None;
        self.one_finger_duration = 0f32;
        self.two_finger_duration = 0f32;
        self.three_finger_duration = 0f32;
        self.four_finger_duration = 0f32;
    }

    pub fn update(&mut self, report: SynReport) {
        let mut reset = false;
        let mut overall_x = None;
        let mut overall_y = None;

        // Loop over events and handle each slot separately
        {
            let mut slot_id = 0usize;
            let mut slot = &mut self.slot_states[0];
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
                        slot_id = event.code as usize;
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
                        self.last_finger = Some(Fingers::One);
                    }
                    (EventType::EV_KEY, EventCode::BTN_TOOL_DOUBLETAP) if event.value == 1 => {
                        self.last_finger = Some(Fingers::Two);
                    }
                    (EventType::EV_KEY, EventCode::BTN_TOOL_TRIPLETAP) if event.value == 1 => {
                        self.last_finger = Some(Fingers::Three);
                    }
                    (EventType::EV_KEY, EventCode::BTN_TOOL_QUINTTAP) if event.value == 1 => {
                        self.last_finger = Some(Fingers::Four);
                    }

                    // Finger state removed
                    // Assuming we never miss an event, the finger should always have started
                    (EventType::EV_KEY, EventCode::BTN_TOOL_FINGER) if event.value == 0 => {
                        self.one_finger_duration += event.time - self.finger_start.unwrap();
                        self.last_finger = None;
                    }
                    (EventType::EV_KEY, EventCode::BTN_TOOL_DOUBLETAP) if event.value == 0 => {
                        self.two_finger_duration += event.time - self.finger_start.unwrap();
                        self.last_finger = None;
                    }
                    (EventType::EV_KEY, EventCode::BTN_TOOL_TRIPLETAP) if event.value == 0 => {
                        self.three_finger_duration += event.time - self.finger_start.unwrap();
                        self.last_finger = None;
                    }
                    (EventType::EV_KEY, EventCode::BTN_TOOL_QUINTTAP) if event.value == 0 => {
                        self.four_finger_duration += event.time - self.finger_start.unwrap();
                        self.last_finger = None;
                    }

                    // Tracking complete event
                    // XXX: This should be handled on a per-slot basis, allowing fingers to
                    // drop out in the middle of a gesture and then figuring out only after
                    // all fingers have been tracked to completion what the deal is.
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
            return;
        }

        if let (Some(x), Some(y)) = (overall_x.take(), overall_y.take()) {
            self.push_position(x, y);
        }

        if self.slot_states.iter().all(|slot| slot.is_none() || slot.as_ref().unwrap().complete) {
            self.process();
            self.reset();
        }
    }

    pub fn push_position(&mut self, x: i32, y: i32) {
        if self.start_xy.is_none() {
            self.start_xy = Some(pos(x, y));
        } else {
            self.end_xy = Some(pos(x, y));
        }
    }

    fn process(&self) -> Option<Gesture> {
        if self.start_xy.is_none() {
            eprintln!("Received report but indeterminate start");
            return None;
        }

        // Determine most likely finger count
        let finger = if self.one_finger_duration > self.two_finger_duration
            && self.one_finger_duration > self.three_finger_duration
            && self.one_finger_duration > self.four_finger_duration
        {
            Fingers::One
        } else if self.two_finger_duration > self.one_finger_duration
            && self.two_finger_duration > self.three_finger_duration
            && self.two_finger_duration > self.four_finger_duration
        {
            Fingers::Two
        } else if self.three_finger_duration > self.one_finger_duration
            && self.three_finger_duration > self.two_finger_duration
            && self.three_finger_duration > self.four_finger_duration
        {
            Fingers::Three
        } else if self.four_finger_duration > self.one_finger_duration
            && self.four_finger_duration > self.two_finger_duration
            && self.four_finger_duration > self.three_finger_duration
        {
            Fingers::Four
        } else {
            eprintln!("Indeterminate action, all finger durations are equal!");
            return None;
        };

        let distance = match &self.end_xy {
            Some(end_xy) => get_distance(self.start_xy.as_ref().unwrap(), &end_xy),
            None => 0f32,
        };

        if distance < MAX_TAP_DISTANCE {
            Some(Gesture::Tap(finger))
        } else {
            Some(Gesture::Swipe(
                finger,
                get_direction(
                    self.start_xy.as_ref().unwrap(),
                    self.end_xy.as_ref().unwrap(),
                ),
            ))
        }
    }
}
