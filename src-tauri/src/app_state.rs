use std::sync::atomic::{AtomicBool, Ordering};

pub struct WindowBehaviorState {
    minimize_to_tray_on_close: AtomicBool,
}

impl Default for WindowBehaviorState {
    fn default() -> Self {
        Self {
            minimize_to_tray_on_close: AtomicBool::new(true),
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
}