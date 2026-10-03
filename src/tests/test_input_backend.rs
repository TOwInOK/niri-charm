use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisRelativeDirection, AxisSource, Device, DeviceCapability,
    Event, InputBackend, InputTime, KeyState, KeyboardKeyEvent, Keycode, PointerAxisEvent,
    TouchCancelEvent, TouchDownEvent, TouchEvent, TouchFrameEvent, TouchMotionEvent, TouchSlot,
    TouchUpEvent, UnusedEvent,
};
use smithay::output::Output;

use crate::input::backend_ext::NiriInputDevice;

pub struct TestInputBackend;

#[derive(PartialEq, Eq, Hash)]
pub struct TestInputDevice;

pub struct TestKeyboardKeyEvent {
    pub time: InputTime,
    pub code: Keycode,
    pub state: KeyState,
    pub count: u32,
}

impl InputBackend for TestInputBackend {
    type Device = TestInputDevice;

    type KeyboardKeyEvent = TestKeyboardKeyEvent;
    type PointerAxisEvent = TestPointerAxisEvent;
    type PointerButtonEvent = UnusedEvent;
    type PointerMotionEvent = UnusedEvent;
    type PointerMotionAbsoluteEvent = UnusedEvent;

    type GestureSwipeBeginEvent = UnusedEvent;
    type GestureSwipeUpdateEvent = UnusedEvent;
    type GestureSwipeEndEvent = UnusedEvent;
    type GesturePinchBeginEvent = UnusedEvent;
    type GesturePinchUpdateEvent = UnusedEvent;
    type GesturePinchEndEvent = UnusedEvent;
    type GestureHoldBeginEvent = UnusedEvent;
    type GestureHoldEndEvent = UnusedEvent;

    type TouchDownEvent = TestTouchEvent;
    type TouchUpEvent = TestTouchEvent;
    type TouchMotionEvent = TestTouchEvent;
    type TouchCancelEvent = TestTouchEvent;
    type TouchFrameEvent = TestTouchEvent;
    type TabletToolAxisEvent = UnusedEvent;
    type TabletToolProximityEvent = UnusedEvent;
    type TabletToolTipEvent = UnusedEvent;
    type TabletToolButtonEvent = UnusedEvent;

    type SwitchToggleEvent = UnusedEvent;

    type SpecialEvent = UnusedEvent;
}

impl Device for TestInputDevice {
    fn id(&self) -> String {
        String::from("test")
    }

    fn name(&self) -> String {
        String::from("test input device")
    }

    fn has_capability(&self, capability: DeviceCapability) -> bool {
        matches!(capability, DeviceCapability::Keyboard)
    }

    fn usb_id(&self) -> Option<(u32, u32)> {
        None
    }

    fn syspath(&self) -> Option<std::path::PathBuf> {
        None
    }
}

impl NiriInputDevice for TestInputDevice {
    fn output(&self, _state: &crate::niri::State) -> Option<Output> {
        None
    }
}

impl Event<TestInputBackend> for TestKeyboardKeyEvent {
    fn time(&self) -> InputTime {
        self.time
    }

    fn device(&self) -> <TestInputBackend as InputBackend>::Device {
        TestInputDevice
    }
}

impl KeyboardKeyEvent<TestInputBackend> for TestKeyboardKeyEvent {
    fn key_code(&self) -> Keycode {
        self.code
    }

    fn state(&self) -> KeyState {
        self.state
    }

    fn count(&self) -> u32 {
        self.count
    }
}

pub struct TestPointerAxisEvent {
    pub source: AxisSource,
    pub axis: Axis,
    pub amount: Option<f64>,
    pub v120: Option<f64>,
}

impl Event<TestInputBackend> for TestPointerAxisEvent {
    fn time(&self) -> InputTime {
        InputTime::from_millis(0)
    }

    fn device(&self) -> TestInputDevice {
        TestInputDevice
    }
}

impl PointerAxisEvent<TestInputBackend> for TestPointerAxisEvent {
    fn amount(&self, axis: Axis) -> Option<f64> {
        (axis == self.axis).then_some(self.amount).flatten()
    }

    fn amount_v120(&self, axis: Axis) -> Option<f64> {
        (axis == self.axis).then_some(self.v120).flatten()
    }

    fn source(&self) -> AxisSource {
        self.source
    }

    fn relative_direction(&self, _: Axis) -> AxisRelativeDirection {
        AxisRelativeDirection::Identical
    }
}

#[derive(Clone, Copy)]
pub struct TestTouchEvent {
    pub slot: TouchSlot,
    /// Normalized position in the device's output, from 0 to 1.
    pub position: (f64, f64),
}

impl Event<TestInputBackend> for TestTouchEvent {
    fn time(&self) -> InputTime {
        InputTime::from_millis(0)
    }

    fn device(&self) -> TestInputDevice {
        TestInputDevice
    }
}

impl TouchEvent<TestInputBackend> for TestTouchEvent {
    fn slot(&self) -> TouchSlot {
        self.slot
    }
}

impl AbsolutePositionEvent<TestInputBackend> for TestTouchEvent {
    fn x(&self) -> f64 {
        self.position.0
    }

    fn y(&self) -> f64 {
        self.position.1
    }

    fn x_transformed(&self, width: i32) -> f64 {
        self.x() * f64::from(width)
    }

    fn y_transformed(&self, height: i32) -> f64 {
        self.y() * f64::from(height)
    }
}

impl TouchDownEvent<TestInputBackend> for TestTouchEvent {}
impl TouchMotionEvent<TestInputBackend> for TestTouchEvent {}
impl TouchUpEvent<TestInputBackend> for TestTouchEvent {}
impl TouchCancelEvent<TestInputBackend> for TestTouchEvent {}
impl TouchFrameEvent<TestInputBackend> for TestTouchEvent {}
