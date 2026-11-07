//! gpui has no custom-shader hook, so these effects are built from its GPU primitives and redrawn
//! every frame. Animation only runs while wanted, so the app otherwise stays idle.

use std::cell::Cell;
use std::collections::VecDeque;
use std::f32::consts::TAU;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationExt, App, BoxShadow, Bounds, Corners, ElementId, Hsla, IntoElement, ParentElement, PathBuilder,
    Pixels, Styled, Window, bounds, canvas, div, ease_in_out, fill, hsla, linear_color_stop, linear_gradient, point, px,
    relative, size,
};

const HISTORY: usize = 120;

pub struct Fx {
    start: Instant,
    pub showcase: bool,
    /// Bumping it changes the sweep's element id, which restarts it.
    pub sweep: u64,
    frames: VecDeque<Instant>,
    cpu: Rc<Cell<Duration>>,
    cpu_avg: f32,
}

impl Fx {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            showcase: false,
            sweep: 0,
            frames: VecDeque::with_capacity(HISTORY + 1),
            cpu: Rc::new(Cell::new(Duration::ZERO)),
            cpu_avg: 0.,
        }
    }

    pub fn toggle_showcase(&mut self) {
        self.showcase = !self.showcase;
        self.frames.clear();
        self.cpu_avg = 0.;
    }

    pub fn time(&self, cx: &App) -> f32 {
        if cx.reduce_motion() { 4.0 } else { self.start.elapsed().as_secs_f32() }
    }

    /// Call at the start of every render.
    pub fn begin_frame(&mut self, animate: bool, window: &mut Window, cx: &App) -> Instant {
        let now = Instant::now();
        self.frames.push_back(now);
        // Only recent, continuous frames count: an idle gap would drag the FPS down.
        while self.frames.len() > HISTORY || self.frames.front().is_some_and(|f| now - *f > Duration::from_secs(2)) {
            self.frames.pop_front();
        }
        let cpu = self.cpu.get().as_secs_f32() * 1000.;
        self.cpu_avg = if self.cpu_avg == 0. { cpu } else { self.cpu_avg * 0.9 + cpu * 0.1 };
        if animate && !cx.reduce_motion() {
            window.request_animation_frame();
        }
        now
    }

    /// Must be last in the tree: measures CPU time up to the end of paint.
    pub fn end_frame(&self, started: Instant) -> impl IntoElement {
        let cpu = self.cpu.clone();
        canvas(|_, _, _| (), move |_, _, _, _| cpu.set(started.elapsed())).absolute().size_0()
    }

    pub fn hud(&self, fg: Hsla, muted: Hsla, accent: Hsla, surface: Hsla) -> impl IntoElement {
        let intervals: Vec<f32> = self
            .frames
            .iter()
            .zip(self.frames.iter().skip(1))
            .map(|(a, b)| (*b - *a).as_secs_f32() * 1000.)
            .collect();
        let span = match (self.frames.front(), self.frames.back()) {
            (Some(a), Some(b)) if self.frames.len() > 1 => (*b - *a).as_secs_f32(),
            _ => 0.,
        };
        let fps = if span > 0. { (self.frames.len() - 1) as f32 / span } else { 0. };
        let avg = if intervals.is_empty() { 0. } else { intervals.iter().sum::<f32>() / intervals.len() as f32 };
        let worst = intervals.iter().copied().fold(0., f32::max);
        let budget = if avg > 0. { (self.cpu_avg / avg * 100.).min(100.) } else { 0. };

        let stat = |value: String, label: &'static str| {
            div()
                .flex()
                .flex_col()
                .child(div().text_color(fg).font_weight(gpui::FontWeight::BOLD).child(value))
                .child(div().text_color(muted).child(label))
        };
        div()
            .absolute()
            .right(px(16.))
            .bottom(px(40.))
            .w(px(300.))
            .p(px(10.))
            .gap(px(8.))
            .flex()
            .flex_col()
            .rounded(px(10.))
            .bg(surface.opacity(0.82))
            .border_1()
            .border_color(accent.opacity(0.5))
            .shadow(vec![BoxShadow {
                color: accent.opacity(0.35),
                offset: point(px(0.), px(4.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .text_xs()
            .font_family("Consolas")
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(stat(format!("{fps:.0}"), "fps"))
                    .child(stat(format!("{avg:.1} ms"), "frame"))
                    .child(stat(format!("{:.2} ms", self.cpu_avg), "cpu/frame"))
                    .child(stat(format!("{budget:.0}%"), "of budget")),
            )
            .child(sparkline(intervals, worst, accent, muted).h(px(36.)).w_full())
            .child(div().text_color(muted).child("Whole UI rebuilt + GPU-drawn every frame · F12 to hide"))
    }
}

fn sparkline(intervals: Vec<f32>, worst: f32, accent: Hsla, muted: Hsla) -> gpui::Canvas<()> {
    canvas(
        |_, _, _| (),
        move |b, _, window, _| {
            window.paint_quad(fill(b, muted.opacity(0.08)));
            if intervals.len() < 2 {
                return;
            }
            let ceiling = worst.max(20.);
            let y60 = b.bottom() - b.size.height * (16.7 / ceiling);
            window.paint_quad(fill(bounds(point(b.left(), y60), size(b.size.width, px(1.))), muted.opacity(0.35)));
            let step = b.size.width / (HISTORY - 1) as f32;
            let x0 = b.right() - step * (intervals.len() - 1) as f32;
            let at = |i: usize, v: f32| point(x0 + step * i as f32, b.bottom() - b.size.height * (v / ceiling).min(1.));
            let mut area = PathBuilder::fill();
            area.move_to(point(x0, b.bottom()));
            for (i, v) in intervals.iter().enumerate() {
                area.line_to(at(i, *v));
            }
            area.line_to(point(b.right(), b.bottom()));
            area.close();
            if let Ok(path) = area.build() {
                window.paint_path(
                    path,
                    linear_gradient(180., linear_color_stop(accent.opacity(0.45), 0.), linear_color_stop(accent.opacity(0.), 1.)),
                );
            }
            let mut line = PathBuilder::stroke(px(1.5));
            line.move_to(at(0, intervals[0]));
            for (i, v) in intervals.iter().enumerate().skip(1) {
                line.line_to(at(i, *v));
            }
            if let Ok(path) = line.build() {
                window.paint_path(path, accent);
            }
        },
    )
}

fn palette(accent: Hsla) -> [Hsla; 6] {
    [
        accent,
        hsla(0.74, 0.84, 0.58, 1.),
        hsla(0.53, 0.70, 0.45, 1.),
        hsla(0.88, 0.73, 0.69, 1.),
        hsla(0.10, 0.90, 0.60, 1.),
        hsla(0.62, 0.80, 0.60, 1.),
    ]
}

/// Place in an `absolute().inset_0()` parent behind the content.
pub fn aurora(t: f32, accent: Hsla, base: Hsla, strength: f32) -> gpui::Canvas<()> {
    canvas(
        |_, _, _| (),
        move |b, _, window, _| paint_aurora(b, t, accent, base, strength, window),
    )
    .size_full()
}

fn paint_aurora(b: Bounds<Pixels>, t: f32, accent: Hsla, base: Hsla, strength: f32, window: &mut Window) {
    let colours = palette(accent);
    let (w, h) = (b.size.width.as_f32(), b.size.height.as_f32());
    let center = b.center();

    let warm = hsla(accent.h, 0.9, 0.93, 1.);
    window.paint_quad(fill(
        b,
        linear_gradient(
            150. + 25. * (t * 0.07).sin(),
            linear_color_stop(base, 0.),
            linear_color_stop(base.blend(warm.opacity(0.6 * strength)), 1.),
        ),
    ));

    let r = w.min(h) * 0.30;
    for (i, colour) in colours.iter().enumerate() {
        let k = i as f32;
        let phase = k * TAU / colours.len() as f32;
        let x = (t * (0.11 + 0.023 * k) + phase).sin() * w * 0.36;
        let y = (t * (0.087 + 0.017 * k) + phase * 1.7).cos() * h * 0.32;
        let breathe = 1. + 0.18 * (t * 0.5 + phase).sin();
        let radius = r * breathe * (0.8 + 0.08 * k);
        let orb = bounds(
            point(center.x + px(x - radius), center.y + px(y - radius)),
            size(px(radius * 2.), px(radius * 2.)),
        );
        let alpha = (0.30 + 0.10 * (t * 0.31 + phase).sin()) * strength;
        window.paint_drop_shadows(
            orb,
            Corners::all(px(radius)),
            &[BoxShadow {
                color: colour.opacity(alpha),
                offset: point(px(0.), px(0.)),
                blur_radius: px(radius * 0.55),
                spread_radius: px(0.),
                inset: false,
            }],
        );
    }

    const SEGMENTS: usize = 64;
    for (i, colour) in colours.iter().take(3).enumerate() {
        let k = i as f32;
        let mid = h * (0.55 + 0.12 * k);
        let edge = |x: f32, off: f32| {
            let u = x / w;
            mid + off
                + h * 0.07 * (u * TAU * (0.8 + 0.3 * k) + t * (0.35 + 0.1 * k) + k).sin()
                + h * 0.03 * (u * TAU * 2.3 - t * (0.6 - 0.1 * k)).sin()
        };
        let thickness = |x: f32| h * (0.05 + 0.035 * ((x / w) * TAU * 1.3 + t * 0.4 + k * 2.).sin());
        let mut path = PathBuilder::fill();
        for s in 0..=SEGMENTS {
            let x = w * s as f32 / SEGMENTS as f32;
            let p = point(b.left() + px(x), b.top() + px(edge(x, 0.)));
            if s == 0 { path.move_to(p) } else { path.line_to(p) }
        }
        for s in (0..=SEGMENTS).rev() {
            let x = w * s as f32 / SEGMENTS as f32;
            path.line_to(point(b.left() + px(x), b.top() + px(edge(x, 0.) + thickness(x))));
        }
        path.close();
        if let Ok(path) = path.build() {
            let next = colours[(i + 1) % colours.len()];
            window.paint_path(
                path,
                linear_gradient(
                    90. + 30. * (t * 0.2 + k).sin(),
                    linear_color_stop(colour.opacity(0.16 * strength), 0.),
                    linear_color_stop(next.opacity(0.05 * strength), 1.),
                ),
            );
        }
    }

    for i in 0..48u32 {
        let hash = |n: u32| ((n.wrapping_mul(2_654_435_761) >> 8) & 0xffff) as f32 / 65535.;
        let (sx, sy, sp) = (hash(i * 3 + 1), hash(i * 3 + 2), hash(i * 3 + 3));
        let twinkle = ((t * (0.8 + sp) + sp * TAU).sin() * 0.5 + 0.5).powi(3);
        if twinkle < 0.05 {
            continue;
        }
        let drift = (t * 6. * (0.5 + sp)) % h;
        let pos = point(b.left() + px(sx * w), b.top() + px((sy * h - drift).rem_euclid(h)));
        let dot = px(1.5 + 2. * sp);
        let spark = bounds(point(pos.x - dot / 2., pos.y - dot / 2.), size(dot, dot));
        window.paint_drop_shadows(
            spark,
            Corners::all(dot),
            &[BoxShadow {
                color: colours[i as usize % colours.len()].opacity(0.9 * twinkle * strength),
                offset: point(px(0.), px(0.)),
                blur_radius: dot * 1.5,
                spread_radius: px(0.),
                inset: false,
            }],
        );
        window.paint_quad(fill(spark, hsla(0., 0., 1., twinkle * strength)).corner_radii(dot));
    }
}

pub fn glow_frame(t: f32, accent: Hsla, radius: Pixels, content: impl IntoElement) -> gpui::Div {
    let mauve = palette(accent)[1];
    let pulse = 0.5 + 0.5 * (t * 1.3).sin();
    div()
        .p(px(1.5))
        .rounded(radius + px(1.5))
        .bg(linear_gradient((t * 40.) % 360., linear_color_stop(accent, 0.), linear_color_stop(mauve, 1.)))
        .shadow(vec![
            BoxShadow {
                color: accent.opacity(0.18 + 0.14 * pulse),
                offset: point(px(0.), px(8.)),
                blur_radius: px(40. + 16. * pulse),
                spread_radius: px(2.),
                inset: false,
            },
            BoxShadow {
                color: mauve.opacity(0.12 + 0.10 * (1. - pulse)),
                offset: point(px(0.), px(-6.)),
                blur_radius: px(56.),
                spread_radius: px(0.),
                inset: false,
            },
        ])
        .child(content)
}

pub fn sweep_bar(sweep: u64, busy: bool, accent: Hsla) -> impl IntoElement {
    let half = |angle: f32| {
        div().flex_1().h_full().bg(linear_gradient(
            angle,
            linear_color_stop(accent.opacity(0.), 0.),
            linear_color_stop(accent, 1.),
        ))
    };
    let streak = div().absolute().top_0().h_full().w(relative(0.35)).flex().child(half(90.)).child(half(270.));
    let (id, animation) = if busy {
        (ElementId::Name("busy-sweep".into()), Animation::new(Duration::from_millis(1100)).repeat())
    } else {
        (ElementId::NamedInteger("sweep".into(), sweep), Animation::new(Duration::from_millis(900)))
    };
    div().relative().w_full().h(px(2.)).overflow_hidden().child(
        streak.with_animation(id, animation.with_easing(ease_in_out), |el, d| el.left(relative(-0.35 + 1.35 * d))),
    )
}
