use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    App, Pixels, Point, ScrollHandle, StatefulInteractiveElement, Window, point, px,
};

/// Zed's editor default from the feat/smooth-scrolling branch
/// (https://github.com/marcocondrache/zed/tree/feat/smooth-scrolling).
const SCROLL_ANIMATION_DURATION: Duration = Duration::from_millis(125);

#[derive(Clone, Copy)]
struct Progress(f32);

impl Progress {
    const COMPLETE: Self = Self(1.0);

    fn remaining(self) -> f32 {
        (1.0 - self.0).max(0.0)
    }

    fn is_finished(self) -> bool {
        self.0 >= 1.0
    }

    fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }
}

enum Animation {
    Completed {
        position: Point<f32>,
    },
    Animating {
        position: Point<f32>,
        start_position: Point<f32>,
        target_position: Point<f32>,
        start_time: Instant,
        duration: Duration,
    },
}

impl Animation {
    fn completed(position: Point<f32>) -> Self {
        Self::Completed { position }
    }

    fn target_position(&self) -> Point<f32> {
        match self {
            Self::Completed { position } => *position,
            Self::Animating {
                target_position, ..
            } => *target_position,
        }
    }

    fn position(&self) -> Point<f32> {
        match self {
            Self::Completed { position } => *position,
            Self::Animating { position, .. } => *position,
        }
    }

    fn is_animating(&self) -> bool {
        matches!(self, Self::Animating { .. })
    }

    fn progress_at(&self, now: Instant) -> Progress {
        match self {
            Self::Completed { .. } => Progress::COMPLETE,
            Self::Animating {
                start_position,
                target_position,
                start_time,
                duration,
                ..
            } => {
                if start_position == target_position {
                    return Progress::COMPLETE;
                }
                let elapsed = now.duration_since(*start_time).as_secs_f32();
                let duration = duration.as_secs_f32().max(f32::EPSILON);
                Progress(elapsed / duration).min(Progress::COMPLETE)
            }
        }
    }

    fn advance_at(&mut self, now: Instant) {
        let Self::Animating {
            start_position,
            target_position,
            ..
        } = *self
        else {
            return;
        };
        let progress = self.progress_at(now);
        if progress.is_finished() {
            *self = Self::Completed {
                position: target_position,
            };
        } else if let Self::Animating { position, .. } = self {
            position.x = interpolate(start_position.x, target_position.x, progress);
            position.y = interpolate(start_position.y, target_position.y, progress);
        }
    }

    fn restart_at(&mut self, target: Point<f32>, now: Instant) {
        self.advance_at(now);
        let current = self.position();
        if current == target {
            *self = Self::Completed { position: target };
            return;
        }
        let duration = match self {
            Self::Animating { .. } => {
                let progress = self.progress_at(now);
                if progress.is_finished() {
                    SCROLL_ANIMATION_DURATION
                } else {
                    self.update_duration_at(target, now)
                }
            }
            Self::Completed { .. } => SCROLL_ANIMATION_DURATION,
        };
        *self = Self::Animating {
            position: current,
            start_position: current,
            target_position: target,
            start_time: now,
            duration,
        };
    }

    fn update_duration_at(&self, new_target: Point<f32>, now: Instant) -> Duration {
        let Self::Animating {
            start_position,
            target_position,
            duration,
            ..
        } = *self
        else {
            return SCROLL_ANIMATION_DURATION;
        };
        let current = self.position();
        let remaining = self.progress_at(now).remaining();
        // Derivative of ease_out_cubic f(t) = 1 - (1-t)^3 is f'(t) = 3(1-t)^2.
        let derivative = 3.0 * remaining * remaining;
        let old_secs = duration.as_secs_f32().max(f32::EPSILON);
        let velocity = point(
            (target_position.x - start_position.x) * derivative / old_secs,
            (target_position.y - start_position.y) * derivative / old_secs,
        );
        let displacement = point(new_target.x - current.x, new_target.y - current.y);
        let (dominant_disp, dominant_vel) = if displacement.x.abs() >= displacement.y.abs() {
            (displacement.x, velocity.x)
        } else {
            (displacement.y, velocity.y)
        };
        if dominant_disp * dominant_vel < 0.0 || dominant_vel.abs() < 1e-6 {
            return SCROLL_ANIMATION_DURATION;
        }
        // At t=0, v0 = displacement * f'(0) / duration = displacement * 3 / duration.
        let new_secs = dominant_disp * 3.0 / dominant_vel;
        let min = SCROLL_ANIMATION_DURATION.as_secs_f32() / 8.0;
        let max = SCROLL_ANIMATION_DURATION.as_secs_f32();
        Duration::from_secs_f32(new_secs.abs().clamp(min, max))
    }
}

fn interpolate(from: f32, to: f32, progress: Progress) -> f32 {
    let eased = ease_out_cubic(progress.0);
    from + (to - from) * eased
}

fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn to_f32(offset: Point<Pixels>) -> Point<f32> {
    point(f32::from(offset.x), f32::from(offset.y))
}

fn to_px(offset: Point<f32>) -> Point<Pixels> {
    point(px(offset.x), px(offset.y))
}

struct Inner {
    animation: Animation,
    ticking: bool,
}

/// Wheel-driven scroll that interpolates with ease-out cubic, retargeting
/// from the current velocity the way the Zed feat/smooth-scrolling branch does.
#[derive(Clone)]
pub struct SmoothScroll {
    handle: ScrollHandle,
    inner: Rc<RefCell<Inner>>,
}

impl Default for SmoothScroll {
    fn default() -> Self {
        Self::new()
    }
}

impl SmoothScroll {
    pub fn new() -> Self {
        Self {
            handle: ScrollHandle::new(),
            inner: Rc::new(RefCell::new(Inner {
                animation: Animation::completed(point(0.0, 0.0)),
                ticking: false,
            })),
        }
    }

    pub fn vertical<E>(&self, element: E) -> E
    where
        E: StatefulInteractiveElement,
    {
        let this = self.clone();
        element
            .overflow_y_scroll()
            .track_scroll(&self.handle)
            .on_scroll_wheel(move |_, window, cx| this.on_wheel(window, cx))
    }

    fn on_wheel(&self, window: &mut Window, cx: &mut App) {
        let after = to_f32(self.handle.offset());
        let visual = self.inner.borrow().animation.position();
        if cx.reduce_motion() {
            let target = self.clamp(after);
            self.handle.set_offset(to_px(target));
            self.inner.borrow_mut().animation = Animation::completed(target);
            return;
        }
        let delta = point(after.x - visual.x, after.y - visual.y);
        let previous_target = self.inner.borrow().animation.target_position();
        let target = self.clamp(point(
            previous_target.x + delta.x,
            previous_target.y + delta.y,
        ));
        self.handle.set_offset(to_px(visual));
        self.inner
            .borrow_mut()
            .animation
            .restart_at(target, Instant::now());
        self.ensure_ticking(window);
    }

    fn clamp(&self, offset: Point<f32>) -> Point<f32> {
        let max = to_f32(self.handle.max_offset());
        let x = if max.x > 0.0 {
            offset.x.clamp(-max.x, 0.0)
        } else {
            offset.x.min(0.0)
        };
        let y = if max.y > 0.0 {
            offset.y.clamp(-max.y, 0.0)
        } else {
            offset.y.min(0.0)
        };
        point(x, y)
    }

    fn ensure_ticking(&self, window: &Window) {
        if self.inner.borrow().ticking {
            return;
        }
        self.inner.borrow_mut().ticking = true;
        self.queue_tick(window);
    }

    fn queue_tick(&self, window: &Window) {
        let this = self.clone();
        window.on_next_frame(move |window, _cx| {
            let mut inner = this.inner.borrow_mut();
            if !inner.animation.is_animating() {
                inner.ticking = false;
                return;
            }
            inner.animation.advance_at(Instant::now());
            let pos = inner.animation.position();
            let still = inner.animation.is_animating();
            drop(inner);
            this.handle.set_offset(to_px(pos));
            window.refresh();
            if still {
                this.queue_tick(window);
            } else {
                this.inner.borrow_mut().ticking = false;
            }
        });
    }
}
