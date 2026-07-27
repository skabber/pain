//! Waking the event loop from a PTY reader thread.
//!
//! The loop sleeps (`ControlFlow::WaitUntil`) rather than spinning, so
//! output arriving on a background thread has to actively nudge it —
//! otherwise a command's output would sit in its channel until the next
//! unrelated event or the next 500ms timer tick.
//!
//! Wakes are **coalesced**. A program producing output at speed (`yes`, a
//! large `cat`) triggers a read every few kilobytes, and winit delivers
//! every proxy event individually — so waking naively would queue
//! thousands of redundant events, which is the same "burn CPU for
//! nothing" problem the sleep was introduced to fix. The flag below means
//! at most one wake is ever in flight: further reads skip sending until
//! the loop has actually woken and cleared it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use winit::event_loop::EventLoopProxy;

/// A cloneable handle a PTY reader thread uses to wake the event loop.
#[derive(Clone)]
pub struct Waker {
    /// `None` for a waker with nothing to wake — see `noop`.
    proxy: Option<EventLoopProxy<()>>,
    pending: Arc<AtomicBool>,
}

impl Waker {
    pub fn new(proxy: EventLoopProxy<()>) -> Self {
        Self { proxy: Some(proxy), pending: Arc::new(AtomicBool::new(false)) }
    }

    /// A waker that does nothing, for spawning a pane outside a running
    /// event loop (tests). An `EventLoopProxy` can only come from a real
    /// `EventLoop`, which a headless test can't create — and a pane whose
    /// output nobody is waiting to render doesn't need to wake anything.
    #[cfg(test)]
    pub fn noop() -> Self {
        Self { proxy: None, pending: Arc::new(AtomicBool::new(false)) }
    }

    /// Signals that there's something to process. Cheap and safe to call
    /// on every read — redundant calls while a wake is already queued do
    /// nothing.
    pub fn wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel)
            && let Some(proxy) = &self.proxy
        {
            // An error here means the loop is shutting down, which is
            // not a problem worth reporting from a reader thread.
            let _ = proxy.send_event(());
        }
    }

    /// Called by the loop once it's awake and about to drain whatever
    /// arrived, re-arming the wake for the next batch.
    pub fn clear(&self) {
        self.pending.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The coalescing guard, which is the whole reason this type exists
    /// rather than calling `send_event` directly: a program producing
    /// output at speed triggers a read every few kilobytes, and winit
    /// delivers every proxy event, so an unguarded wake would replace the
    /// busy render loop with a busy event loop.
    #[test]
    fn only_the_first_wake_is_sent_until_the_loop_clears_it() {
        let waker = Waker::noop();
        assert!(!waker.pending.load(Ordering::Acquire), "starts un-armed");

        waker.wake();
        assert!(waker.pending.load(Ordering::Acquire), "first wake arms the flag");

        // Any number of further wakes while one is already in flight must
        // not queue more work.
        for _ in 0..1000 {
            waker.wake();
        }
        assert!(waker.pending.load(Ordering::Acquire));

        waker.clear();
        assert!(!waker.pending.load(Ordering::Acquire), "clearing re-arms for the next batch");
        waker.wake();
        assert!(waker.pending.load(Ordering::Acquire), "and the next wake goes through");
    }

    #[test]
    fn cloned_wakers_share_one_pending_flag() {
        // Every pane's reader thread gets its own clone; they must not
        // each be able to queue a wake independently.
        let a = Waker::noop();
        let b = a.clone();
        a.wake();
        assert!(b.pending.load(Ordering::Acquire));
        b.clear();
        assert!(!a.pending.load(Ordering::Acquire));
    }
}
