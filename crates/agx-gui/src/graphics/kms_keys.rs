//! Slint 1.18 LinuxKMS maps Meta_L/R but omits XKB Super_L/R.
//! Translate just the two physical Super keys through its public libinput hook.
//! The remaining keyboard layout, modifiers and repeat handling stay in Slint.
use slint::platform::{Key, WindowEvent};

fn super_event(code: u32, down: bool) -> Option<WindowEvent> {
    // Linux input-event-codes.h: KEY_LEFTMETA / KEY_RIGHTMETA.
    let text = match code {
        125 => Key::Meta.into(),
        126 => Key::MetaR.into(),
        _ => return None,
    };
    Some(if down {
        WindowEvent::KeyPressed { text }
    } else {
        WindowEvent::KeyReleased { text }
    })
}

#[cfg(feature = "kms")]
mod native {
    use super::super_event;
    use input::{
        Event,
        event::{
            KeyboardEvent,
            keyboard::{KeyState, KeyboardEventTrait},
        },
    };
    use slint::{ComponentHandle, platform::WindowEvent};
    use std::{cell::RefCell, rc::Rc};
    type Target = Rc<dyn Fn(WindowEvent)>;
    thread_local! { static TARGET: RefCell<Option<Target>> = RefCell::new(None); }
    pub(in crate::graphics) fn attach<C: ComponentHandle + 'static>(ui: &C) {
        let weak = ui.as_weak();
        TARGET.with(|target| {
            *target.borrow_mut() = Some(Rc::new(move |event| {
                if let Some(ui) = weak.upgrade() {
                    ui.window().dispatch_event(event);
                }
            }))
        });
    }
    pub(in crate::graphics) fn filter(event: &Event) -> bool {
        let Event::Keyboard(KeyboardEvent::Key(key)) = event else {
            return false;
        };
        let Some(event) = super_event(key.key(), key.key_state() == KeyState::Pressed) else {
            return false;
        };
        // The hook runs on the UI thread. Release the borrow before dispatching callbacks.
        let target = TARGET.with(|target| target.borrow().clone());
        if let Some(target) = target {
            target(event);
            return true;
        }
        false
    }
}
#[cfg(feature = "kms")]
pub(super) use native::{attach, filter};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_super_keys_have_matching_press_and_release_events() {
        for (code, key) in [(125, Key::Meta), (126, Key::MetaR)] {
            assert!(
                matches!(super_event(code, true), Some(WindowEvent::KeyPressed { text }) if text == slint::SharedString::from(key))
            );
            assert!(
                matches!(super_event(code, false), Some(WindowEvent::KeyReleased { text }) if text == slint::SharedString::from(key))
            );
        }
        for code in [0, 30, 42, 54, 56, 57, 97, 100] {
            assert!(super_event(code, true).is_none());
        }
    }
    #[test]
    fn physical_super_space_sets_and_clears_the_ui_modifier() {
        use slint::{
            ComponentHandle,
            platform::{
                Platform, PlatformError, WindowAdapter,
                software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
            },
        };
        use std::{cell::RefCell, rc::Rc};
        struct TestPlatform(Rc<MinimalSoftwareWindow>);
        impl Platform for TestPlatform {
            fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
                Ok(self.0.clone())
            }
        }
        let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        slint::platform::set_platform(Box::new(TestPlatform(window))).unwrap();
        let ui = crate::AppWindow::new().unwrap();
        ui.show().unwrap();
        ui.invoke_focus_desktop();
        let modifiers = Rc::new(RefCell::new(Vec::new()));
        let received = modifiers.clone();
        ui.on_key(move |name, _, _, _, meta| {
            if name == "space" {
                received.borrow_mut().push(meta);
            }
            true
        });
        for code in [125, 126] {
            ui.window().dispatch_event(super_event(code, true).unwrap());
            ui.window()
                .dispatch_event(WindowEvent::KeyPressed { text: " ".into() });
            ui.window()
                .dispatch_event(WindowEvent::KeyReleased { text: " ".into() });
            ui.window()
                .dispatch_event(super_event(code, false).unwrap());
            ui.window()
                .dispatch_event(WindowEvent::KeyPressed { text: " ".into() });
            ui.window()
                .dispatch_event(WindowEvent::KeyReleased { text: " ".into() });
        }
        assert_eq!(*modifiers.borrow(), vec![true, false, true, false]);
    }
}
