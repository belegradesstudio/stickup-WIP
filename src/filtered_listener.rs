use crate::devices::event::InputEvent;
use crate::devices::eventbus::InputListener;

/// Wraps a listener and filters events based on a user-supplied predicate.
pub struct FilteredListener {
    predicate: Box<dyn Fn(&InputEvent) -> bool + Send + Sync>,
    inner: Box<dyn InputListener>,
}

impl FilteredListener {
    pub fn new(
        predicate: impl Fn(&InputEvent) -> bool + Send + Sync + 'static,
        inner: Box<dyn InputListener>,
    ) -> Self {
        Self {
            predicate: Box::new(predicate),
            inner,
        }
    }
}

impl InputListener for FilteredListener {
    fn on_input(&mut self, event: &InputEvent) {
        if (self.predicate)(event) {
            self.inner.on_input(event);
        }
    }
}
