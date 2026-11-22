use std::sync::atomic::{AtomicBool, Ordering};

pub struct WindowBehaviorState {
    minimize_to_tray_on_close: AtomicBool,
    always_on_top: AtomicBool,
}

impl Default for WindowBehaviorState {
    fn default() -> Self {
        Self {
            minimize_to_tray_on_close: AtomicBool::new(true),
            always_on_top: AtomicBool::new(false),
        }
    }
}

impl WindowBehaviorState {
    pub fn get(&self) -> bool {
        self.minimize_to_tray_on_close.load(Ordering::Relaxed)
    }

    pub fn set(&self, value: bool) {
        self.minimize_to_tray_on_close.store(value, Ordering::Relaxed)
    }

    pub fn always_on_top(&self) -> bool {
        self.always_on_top.load(Ordering::Relaxed)
    }

    pub fn set_always_on_top(&self, value: bool) {
        self.always_on_top.store(value, Ordering::Relaxed)
    }
}