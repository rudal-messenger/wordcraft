//! Transient ribbon motion, shared by pointer, keyboard and contextual tab changes.
//! Layout changes snap; only an actual tab change animates. Nothing is persisted.

use egui::{Rect, pos2, vec2};

const DURATION: f64 = 0.22;
const TRAVEL: f32 = 20.0;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    pub underline: Option<Rect>,
    pub offset: f32,
    pub opacity: f32,
    pub moving: bool,
}

#[derive(Clone)]
struct Motion {
    tab: String,
    index: usize,
    from: Option<Rect>,
    to: Option<Rect>,
    started: f64,
    direction: f32,
    content_visible: bool,
    animate_content: bool,
    last_frame: u64,
}

impl Motion {
    fn sample(&self, now: f64) -> Frame {
        let progress = ((now - self.started) / DURATION).clamp(0.0, 1.0) as f32;
        let ease = 1.0 - (1.0 - progress).powi(3);
        let underline = match (self.from, self.to) {
            (Some(from), Some(to)) => Some(Rect::from_min_max(
                pos2(egui::lerp(from.left()..=to.left(), ease), to.top()),
                pos2(egui::lerp(from.right()..=to.right(), ease), to.bottom()),
            )),
            (_, to) => to,
        };
        Frame {
            underline,
            offset: if self.animate_content { self.direction * TRAVEL * (1.0 - ease) } else { 0.0 },
            opacity: if self.animate_content { 0.45 + 0.55 * ease } else { 1.0 },
            moving: progress < 1.0,
        }
    }
}

pub(crate) fn frame(ctx: &egui::Context, tab: &str, index: usize, target: Option<Rect>, enabled: bool, content_visible: bool) -> Frame {
    let id = egui::Id::new("ribbon_tab_motion");
    let now = ctx.input(|i| i.time);
    let frame_nr = ctx.cumulative_frame_nr();
    let previous = ctx.data_mut(|d| d.get_temp::<Motion>(id));
    let mut motion = match previous {
        Some(mut previous) if enabled && frame_nr <= previous.last_frame.saturating_add(1) => {
            if previous.tab != tab {
                // Retarget the underline from its current position even during rapid clicks.
                previous.from = previous.sample(now).underline;
                previous.to = target;
                previous.started = now;
                previous.direction = if index >= previous.index { 1.0 } else { -1.0 };
                previous.animate_content = previous.content_visible && content_visible;
            } else if previous.to != target || previous.content_visible != content_visible {
                previous.from = target;
                previous.to = target;
                previous.started = now - DURATION - 1.0;
            }
            previous
        }
        _ => Motion {
            tab: tab.to_owned(),
            index,
            from: target,
            to: target,
            started: now - DURATION - 1.0,
            direction: 1.0,
            content_visible,
            animate_content: false,
            last_frame: frame_nr,
        },
    };
    let result = motion.sample(now);
    motion.tab = tab.to_owned();
    motion.index = index;
    motion.content_visible = content_visible;
    motion.last_frame = frame_nr;
    ctx.data_mut(|d| d.insert_temp(id, motion));
    if result.moving {
        ctx.request_repaint();
    }
    result
}

pub(crate) fn paint_underline(ui: &egui::Ui, frame: Frame, color: egui::Color32) {
    if let Some(rect) = frame.underline {
        ui.painter().rect_filled(rect, 2.0, color);
    }
}

pub(crate) fn content_ui(ui: &mut egui::Ui, frame: Frame) -> egui::Ui {
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt("ribbon_motion_content").max_rect(ui.max_rect().translate(vec2(frame.offset, 0.0))));
    // Keep the viewport fixed while the controls move, including their hit rectangles.
    child.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
    child.set_opacity(frame.opacity);
    child
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_retargets_and_reduced_motion_snaps() {
        let ctx = egui::Context::default();
        let a = Rect::from_min_size(pos2(10.0, 27.0), vec2(40.0, 3.0));
        let b = a.translate(vec2(80.0, 0.0));
        let mut output = None;
        let mut step = |time, tab, index, target, enabled| {
            ctx.run_ui(egui::RawInput { time: Some(time), ..Default::default() }, |ui| {
                output = Some(frame(ui.ctx(), tab, index, Some(target), enabled, true));
            })
            .drop_without_applying_deltas();
            output.unwrap()
        };
        let initial = step(0.0, "Home", 0, a, true);
        assert!(!initial.moving && initial.offset == 0.0 && initial.opacity == 1.0);
        let start = step(0.1, "Insert", 1, b, true);
        assert_eq!(start.underline, Some(a));
        assert!(start.offset > 0.0 && start.opacity < 1.0);
        let middle = step(0.2, "Insert", 1, b, true);
        assert!(middle.underline.unwrap().left() > a.left() && middle.underline.unwrap().left() < b.left());
        let reverse = step(0.2, "Home", 0, a, true);
        assert_eq!(reverse.underline, middle.underline);
        assert!(reverse.offset < 0.0);
        let settled = step(0.5, "Home", 0, a, true);
        assert_eq!(settled.underline, Some(a));
        assert!(!settled.moving && settled.offset == 0.0 && settled.opacity == 1.0);
        step(0.51, "Insert", 1, b, true);
        let off = step(0.52, "Insert", 1, b, false);
        assert_eq!(off.underline, Some(b));
        assert!(!off.moving && off.offset == 0.0 && off.opacity == 1.0);
        let resized = step(0.53, "Insert", 1, a, true);
        assert_eq!(resized.underline, Some(a));
        assert!(!resized.moving);
        let done = step(0.6, "Insert", 1, a, true);
        assert!(!done.moving && done.opacity == 1.0);
    }

    #[test]
    fn hidden_ribbon_does_not_replay_a_transition_on_return() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(pos2(10.0, 27.0), vec2(40.0, 3.0));
        let draw = |time, tab, visible| {
            let mut result = None;
            ctx.run_ui(egui::RawInput { time: Some(time), ..Default::default() }, |ui| {
                result = Some(frame(ui.ctx(), tab, 0, Some(rect), true, visible));
            })
            .drop_without_applying_deltas();
            result.unwrap()
        };
        draw(0.0, "Home", true);
        draw(0.1, "Home", false);
        let collapsed = draw(0.2, "Review", false);
        assert_eq!((collapsed.offset, collapsed.opacity), (0.0, 1.0));
        let opened = draw(0.21, "Review", true);
        assert!(!opened.moving && opened.offset == 0.0 && opened.opacity == 1.0);
        // Backstage and Focus omit the ribbon entirely for several UI frames.
        for _ in 0..3 {
            ctx.run_ui(Default::default(), |_| {}).drop_without_applying_deltas();
        }
        let returned = draw(0.3, "Insert", true);
        assert!(!returned.moving && returned.offset == 0.0 && returned.opacity == 1.0);
    }

    #[test]
    fn content_moves_its_hit_boxes_without_changing_width_or_clip() {
        let ctx = egui::Context::default();
        ctx.run_ui(Default::default(), |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let viewport = ui.max_rect();
                let clip = ui.clip_rect().intersect(viewport);
                let motion = Frame { underline: None, offset: -20.0, opacity: 0.6, moving: true };
                let mut content = content_ui(ui, motion);
                assert_eq!(content.max_rect().width(), viewport.width());
                assert_eq!(content.clip_rect(), clip);
                let button = content.button("Insert table");
                assert_eq!(button.rect.left(), viewport.left() - 20.0);
                assert_eq!(ui.cursor().left(), viewport.left());
            });
        })
        .drop_without_applying_deltas();
    }
}
