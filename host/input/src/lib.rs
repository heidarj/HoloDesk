use std::{
    collections::BTreeSet,
    error::Error,
    fmt,
};

use holobridge_capture::DesktopBounds;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PointerButton {
    Left,
    Middle,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonPhase {
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyPhase {
    Down,
    Up,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    UnsupportedPlatform,
    InvalidButton(String),
    InvalidPhase(String),
    UnsupportedKeyCode(u16),
    WindowsApi {
        operation: &'static str,
        detail: String,
    },
}

pub trait InputBackend: Send {
    fn move_pointer(
        &mut self,
        desktop_x: i32,
        desktop_y: i32,
    ) -> Result<(), InputError>;

    fn button(
        &mut self,
        button: PointerButton,
        phase: ButtonPhase,
    ) -> Result<(), InputError>;

    fn wheel(
        &mut self,
        delta_x: i32,
        delta_y: i32,
    ) -> Result<(), InputError>;

    fn key(
        &mut self,
        key_code: u16,
        phase: KeyPhase,
    ) -> Result<(), InputError>;
}

pub struct InputSession {
    display_bounds: DesktopBounds,
    input_focus_active: bool,
    pressed_buttons: BTreeSet<PointerButton>,
    pressed_keys: BTreeSet<u16>,
    last_pointer_sequence: u64,
    backend: Box<dyn InputBackend>,
}

impl PointerButton {
    pub fn parse(value: &str) -> Result<Self, InputError> {
        match value {
            "left" => Ok(Self::Left),
            "middle" => Ok(Self::Middle),
            "right" => Ok(Self::Right),
            _ => Err(InputError::InvalidButton(value.to_owned())),
        }
    }
}

impl ButtonPhase {
    pub fn parse(value: &str) -> Result<Self, InputError> {
        match value {
            "down" => Ok(Self::Down),
            "up" => Ok(Self::Up),
            _ => Err(InputError::InvalidPhase(value.to_owned())),
        }
    }
}

impl KeyPhase {
    pub fn parse(value: &str) -> Result<Self, InputError> {
        match value {
            "down" => Ok(Self::Down),
            "up" => Ok(Self::Up),
            _ => Err(InputError::InvalidPhase(value.to_owned())),
        }
    }
}

impl InputSession {
    pub fn new(display_bounds: DesktopBounds) -> Result<Self, InputError> {
        Ok(Self::with_backend(
            display_bounds,
            Box::new(PlatformInputBackend::new()?),
        ))
    }

    pub fn with_backend(
        display_bounds: DesktopBounds,
        backend: Box<dyn InputBackend>,
    ) -> Self {
        Self {
            display_bounds,
            input_focus_active: true,
            pressed_buttons: BTreeSet::new(),
            pressed_keys: BTreeSet::new(),
            last_pointer_sequence: 0,
            backend,
        }
    }

    pub fn handle_pointer_motion(
        &mut self,
        x: i32,
        y: i32,
        sequence: u64,
    ) -> Result<(), InputError> {
        if !self.input_focus_active || sequence <= self.last_pointer_sequence {
            return Ok(());
        }

        let (desktop_x, desktop_y) = self.desktop_point(x, y);
        self.backend.move_pointer(desktop_x, desktop_y)?;
        self.last_pointer_sequence = sequence;
        Ok(())
    }

    pub fn handle_pointer_button(
        &mut self,
        button: PointerButton,
        phase: ButtonPhase,
        x: i32,
        y: i32,
        sequence: u64,
    ) -> Result<(), InputError> {
        if !self.input_focus_active {
            return Ok(());
        }

        let (desktop_x, desktop_y) = self.desktop_point(x, y);
        self.backend.move_pointer(desktop_x, desktop_y)?;
        self.last_pointer_sequence = self.last_pointer_sequence.max(sequence);

        match phase {
            ButtonPhase::Down => {
                if self.pressed_buttons.insert(button) {
                    self.backend.button(button, ButtonPhase::Down)?;
                }
            }
            ButtonPhase::Up => {
                if self.pressed_buttons.remove(&button) {
                    self.backend.button(button, ButtonPhase::Up)?;
                }
            }
        }

        Ok(())
    }

    pub fn handle_wheel(
        &mut self,
        delta_x: i32,
        delta_y: i32,
        x: i32,
        y: i32,
        sequence: u64,
    ) -> Result<(), InputError> {
        if !self.input_focus_active {
            return Ok(());
        }

        let (desktop_x, desktop_y) = self.desktop_point(x, y);
        self.backend.move_pointer(desktop_x, desktop_y)?;
        self.last_pointer_sequence = self.last_pointer_sequence.max(sequence);

        if delta_x != 0 || delta_y != 0 {
            self.backend.wheel(delta_x, delta_y)?;
        }

        Ok(())
    }

    pub fn handle_key(
        &mut self,
        key_code: u16,
        phase: KeyPhase,
    ) -> Result<(), InputError> {
        if !self.input_focus_active {
            return Ok(());
        }

        match phase {
            KeyPhase::Down => {
                if self.pressed_keys.insert(key_code) {
                    self.backend.key(key_code, KeyPhase::Down)?;
                }
            }
            KeyPhase::Up => {
                if self.pressed_keys.remove(&key_code) {
                    self.backend.key(key_code, KeyPhase::Up)?;
                }
            }
        }

        Ok(())
    }

    pub fn set_input_focus(
        &mut self,
        active: bool,
    ) -> Result<(), InputError> {
        if self.input_focus_active == active {
            return Ok(());
        }

        if !active {
            self.release_all()?;
        }
        self.input_focus_active = active;
        Ok(())
    }

    pub fn release_all(&mut self) -> Result<(), InputError> {
        let pressed_buttons = self.pressed_buttons.iter().copied().collect::<Vec<_>>();
        for button in pressed_buttons {
            self.backend.button(button, ButtonPhase::Up)?;
            self.pressed_buttons.remove(&button);
        }

        let pressed_keys = self.pressed_keys.iter().copied().collect::<Vec<_>>();
        for key_code in pressed_keys {
            self.backend.key(key_code, KeyPhase::Up)?;
            self.pressed_keys.remove(&key_code);
        }

        Ok(())
    }

    pub fn display_bounds(&self) -> DesktopBounds {
        self.display_bounds
    }

    fn desktop_point(
        &self,
        x: i32,
        y: i32,
    ) -> (i32, i32) {
        let width = self.display_bounds.width().saturating_sub(1) as i32;
        let height = self.display_bounds.height().saturating_sub(1) as i32;
        let clamped_x = x.clamp(0, width.max(0));
        let clamped_y = y.clamp(0, height.max(0));
        (
            self.display_bounds.left + clamped_x,
            self.display_bounds.top + clamped_y,
        )
    }
}

#[cfg(not(windows))]
#[derive(Default)]
struct PlatformInputBackend;

#[cfg(not(windows))]
impl PlatformInputBackend {
    fn new() -> Result<Self, InputError> {
        Ok(Self)
    }
}

#[cfg(not(windows))]
impl InputBackend for PlatformInputBackend {
    fn move_pointer(
        &mut self,
        _desktop_x: i32,
        _desktop_y: i32,
    ) -> Result<(), InputError> {
        Err(InputError::UnsupportedPlatform)
    }

    fn button(
        &mut self,
        _button: PointerButton,
        _phase: ButtonPhase,
    ) -> Result<(), InputError> {
        Err(InputError::UnsupportedPlatform)
    }

    fn wheel(
        &mut self,
        _delta_x: i32,
        _delta_y: i32,
    ) -> Result<(), InputError> {
        Err(InputError::UnsupportedPlatform)
    }

    fn key(
        &mut self,
        _key_code: u16,
        _phase: KeyPhase,
    ) -> Result<(), InputError> {
        Err(InputError::UnsupportedPlatform)
    }
}

#[cfg(windows)]
struct PlatformInputBackend;

#[cfg(windows)]
impl PlatformInputBackend {
    fn new() -> Result<Self, InputError> {
        Ok(Self)
    }
}

#[cfg(windows)]
impl InputBackend for PlatformInputBackend {
    fn move_pointer(
        &mut self,
        desktop_x: i32,
        desktop_y: i32,
    ) -> Result<(), InputError> {
        use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;

        unsafe {
            SetCursorPos(desktop_x, desktop_y)
                .map_err(|error| InputError::WindowsApi {
                    operation: "SetCursorPos",
                    detail: error.message(),
                })
        }
    }

    fn button(
        &mut self,
        button: PointerButton,
        phase: ButtonPhase,
    ) -> Result<(), InputError> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
            MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN,
            MOUSEEVENTF_RIGHTUP, MOUSEINPUT,
        };

        let flags = match (button, phase) {
            (PointerButton::Left, ButtonPhase::Down) => MOUSEEVENTF_LEFTDOWN,
            (PointerButton::Left, ButtonPhase::Up) => MOUSEEVENTF_LEFTUP,
            (PointerButton::Middle, ButtonPhase::Down) => MOUSEEVENTF_MIDDLEDOWN,
            (PointerButton::Middle, ButtonPhase::Up) => MOUSEEVENTF_MIDDLEUP,
            (PointerButton::Right, ButtonPhase::Down) => MOUSEEVENTF_RIGHTDOWN,
            (PointerButton::Right, ButtonPhase::Up) => MOUSEEVENTF_RIGHTUP,
        };

        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if sent == 0 {
            let error = windows::core::Error::from_win32();
            return Err(InputError::WindowsApi {
                operation: "SendInput(mouse button)",
                detail: error.message(),
            });
        }

        Ok(())
    }

    fn wheel(
        &mut self,
        delta_x: i32,
        delta_y: i32,
    ) -> Result<(), InputError> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_WHEEL,
            MOUSEINPUT,
        };

        let mut inputs = Vec::new();
        if delta_y != 0 {
            inputs.push(INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: delta_y as u32,
                        dwFlags: MOUSEEVENTF_WHEEL,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }
        if delta_x != 0 {
            inputs.push(INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: delta_x as u32,
                        dwFlags: MOUSEEVENTF_HWHEEL,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }

        if inputs.is_empty() {
            return Ok(());
        }

        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent == 0 {
            let error = windows::core::Error::from_win32();
            return Err(InputError::WindowsApi {
                operation: "SendInput(mouse wheel)",
                detail: error.message(),
            });
        }

        Ok(())
    }

    fn key(
        &mut self,
        key_code: u16,
        phase: KeyPhase,
    ) -> Result<(), InputError> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
            KEYEVENTF_KEYUP, VIRTUAL_KEY,
        };

        let virtual_key = map_hid_usage_to_virtual_key(key_code)?;
        let flags = match phase {
            KeyPhase::Down => KEYBD_EVENT_FLAGS(0),
            KeyPhase::Up => KEYEVENTF_KEYUP,
        };

        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(virtual_key),
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if sent == 0 {
            let error = windows::core::Error::from_win32();
            return Err(InputError::WindowsApi {
                operation: "SendInput(keyboard)",
                detail: error.message(),
            });
        }

        Ok(())
    }
}

#[cfg(windows)]
fn map_hid_usage_to_virtual_key(key_code: u16) -> Result<u16, InputError> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_APPS, VK_BACK, VK_CAPITAL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F10,
        VK_F11, VK_F12, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_HOME,
        VK_INSERT, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_NEXT, VK_PRIOR,
        VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SPACE, VK_TAB,
        VK_UP,
    };

    let value = match key_code {
        4..=29 => 0x41 + (key_code - 4),
        30..=38 => 0x31 + (key_code - 30),
        39 => 0x30,
        40 => VK_RETURN.0,
        41 => VK_ESCAPE.0,
        42 => VK_BACK.0,
        43 => VK_TAB.0,
        44 => VK_SPACE.0,
        45 => 0xBD,
        46 => 0xBB,
        47 => 0xDB,
        48 => 0xDD,
        49 => 0xDC,
        50 => 0xE2,
        51 => 0xBA,
        52 => 0xDE,
        53 => 0xC0,
        54 => 0xBC,
        55 => 0xBE,
        56 => 0xBF,
        57 => VK_CAPITAL.0,
        58 => VK_F1.0,
        59 => VK_F2.0,
        60 => VK_F3.0,
        61 => VK_F4.0,
        62 => VK_F5.0,
        63 => VK_F6.0,
        64 => VK_F7.0,
        65 => VK_F8.0,
        66 => VK_F9.0,
        67 => VK_F10.0,
        68 => VK_F11.0,
        69 => VK_F12.0,
        73 => VK_INSERT.0,
        74 => VK_HOME.0,
        75 => VK_PRIOR.0,
        76 => VK_DELETE.0,
        77 => VK_END.0,
        78 => VK_NEXT.0,
        79 => VK_RIGHT.0,
        80 => VK_LEFT.0,
        81 => VK_DOWN.0,
        82 => VK_UP.0,
        84 => 0x6F,
        85 => 0x6A,
        86 => 0x6D,
        87 => 0x6B,
        89 => 0x61,
        90 => 0x62,
        91 => 0x63,
        92 => 0x64,
        93 => 0x65,
        94 => 0x66,
        95 => 0x67,
        96 => 0x68,
        97 => 0x69,
        98 => 0x60,
        99 => 0x6E,
        101 => VK_APPS.0,
        224 => VK_LCONTROL.0,
        225 => VK_LSHIFT.0,
        226 => VK_LMENU.0,
        227 => VK_LWIN.0,
        228 => VK_RCONTROL.0,
        229 => VK_RSHIFT.0,
        230 => VK_RMENU.0,
        231 => VK_RWIN.0,
        _ => return Err(InputError::UnsupportedKeyCode(key_code)),
    };

    Ok(value)
}

impl fmt::Display for InputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => formatter.write_str("input replay is unsupported on this platform"),
            Self::InvalidButton(button) => write!(formatter, "invalid pointer button: {button}"),
            Self::InvalidPhase(phase) => write!(formatter, "invalid input phase: {phase}"),
            Self::UnsupportedKeyCode(key_code) => write!(formatter, "unsupported hardware key code: {key_code}"),
            Self::WindowsApi { operation, detail } => {
                write!(formatter, "{operation} failed: {detail}")
            }
        }
    }
}

impl Error for InputError {}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum RecordedAction {
        Move(i32, i32),
        Button(PointerButton, ButtonPhase),
        Wheel(i32, i32),
        Key(u16, KeyPhase),
    }

    struct RecordingBackend {
        actions: Arc<Mutex<Vec<RecordedAction>>>,
    }

    impl RecordingBackend {
        fn new(actions: Arc<Mutex<Vec<RecordedAction>>>) -> Self {
            Self { actions }
        }
    }

    impl InputBackend for RecordingBackend {
        fn move_pointer(
            &mut self,
            desktop_x: i32,
            desktop_y: i32,
        ) -> Result<(), InputError> {
            self.actions
                .lock()
                .unwrap()
                .push(RecordedAction::Move(desktop_x, desktop_y));
            Ok(())
        }

        fn button(
            &mut self,
            button: PointerButton,
            phase: ButtonPhase,
        ) -> Result<(), InputError> {
            self.actions
                .lock()
                .unwrap()
                .push(RecordedAction::Button(button, phase));
            Ok(())
        }

        fn wheel(
            &mut self,
            delta_x: i32,
            delta_y: i32,
        ) -> Result<(), InputError> {
            self.actions
                .lock()
                .unwrap()
                .push(RecordedAction::Wheel(delta_x, delta_y));
            Ok(())
        }

        fn key(
            &mut self,
            key_code: u16,
            phase: KeyPhase,
        ) -> Result<(), InputError> {
            self.actions
                .lock()
                .unwrap()
                .push(RecordedAction::Key(key_code, phase));
            Ok(())
        }
    }

    fn test_bounds() -> DesktopBounds {
        DesktopBounds {
            left: 100,
            top: 50,
            right: 500,
            bottom: 350,
        }
    }

    #[test]
    fn pointer_motion_clamps_to_capture_bounds() {
        let actions = Arc::new(Mutex::new(Vec::new()));
        let mut session = InputSession::with_backend(
            test_bounds(),
            Box::new(RecordingBackend::new(Arc::clone(&actions))),
        );

        session.handle_pointer_motion(999, -50, 1).unwrap();

        assert_eq!(
            *actions.lock().unwrap(),
            vec![RecordedAction::Move(499, 50)]
        );
    }

    #[test]
    fn input_focus_false_releases_pressed_inputs() {
        let actions = Arc::new(Mutex::new(Vec::new()));
        let mut session = InputSession::with_backend(
            test_bounds(),
            Box::new(RecordingBackend::new(Arc::clone(&actions))),
        );

        session
            .handle_pointer_button(PointerButton::Left, ButtonPhase::Down, 10, 20, 1)
            .unwrap();
        session.handle_key(4, KeyPhase::Down).unwrap();
        session.set_input_focus(false).unwrap();

        assert_eq!(
            *actions.lock().unwrap(),
            vec![
                RecordedAction::Move(110, 70),
                RecordedAction::Button(PointerButton::Left, ButtonPhase::Down),
                RecordedAction::Key(4, KeyPhase::Down),
                RecordedAction::Button(PointerButton::Left, ButtonPhase::Up),
                RecordedAction::Key(4, KeyPhase::Up),
            ]
        );
    }

    #[test]
    fn stale_pointer_motion_datagrams_are_ignored() {
        let actions = Arc::new(Mutex::new(Vec::new()));
        let mut session = InputSession::with_backend(
            test_bounds(),
            Box::new(RecordingBackend::new(Arc::clone(&actions))),
        );

        session.handle_pointer_motion(10, 20, 5).unwrap();
        session.handle_pointer_motion(30, 40, 4).unwrap();

        assert_eq!(
            *actions.lock().unwrap(),
            vec![RecordedAction::Move(110, 70)]
        );
    }
}
