//! Presentation controls shared by the instrument panel and its native toolbar.
use eframe::egui::{self, Align2, Color32, FontId, Response, Sense, Stroke, Vec2};

const ACTIVE: Color32 = Color32::from_rgb(218, 83, 92);
const INK: Color32 = Color32::from_rgb(38, 43, 48);
const FOCUS: Color32 = Color32::from_rgb(199, 225, 243);

/// Keep a complete click observable even when down/up arrive between frames,
/// or the button was invoked through keyboard/accessibility. A mouse release
/// after an already sounding held note must still release that note promptly.
pub fn note_key_active(response: &Response, previously_held: bool) -> bool {
    (response.ctx.input(|i| i.focused) && response.is_pointer_button_down_on())
        || (response.clicked() && !previously_held)
}

pub fn note_transitions<'a>(
    previous: &'a [u8],
    current: &'a [u8],
) -> impl Iterator<Item = (u8, bool)> + 'a {
    previous
        .iter()
        .filter(|note| !current.contains(note))
        .map(|&note| (note, false))
        .chain(
            current
                .iter()
                .filter(|note| !previous.contains(note))
                .map(|&note| (note, true)),
        )
}

/// A toggle whose on/off state stays legible without relying on the tiny
/// default checkbox mark. The label identifies the controlled signal path.
pub fn power_toggle(ui: &mut egui::Ui, value: &mut bool, label: &str) -> Response {
    let text = format!("{}  {}", label, if *value { "ON" } else { "OFF" });
    let mut response = ui.add(
        egui::Button::new(egui::RichText::new(text).strong().color(if *value {
            Color32::WHITE
        } else {
            Color32::from_rgb(229, 231, 233)
        }))
        .fill(if *value {
            ACTIVE
        } else {
            Color32::from_rgb(55, 60, 65)
        })
        .stroke(Stroke::new(
            1.0,
            if *value {
                ACTIVE
            } else {
                Color32::from_gray(126)
            },
        ))
        .min_size(egui::vec2(88.0, 32.0)),
    );
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, label)
    });
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            4.0,
            Stroke::new(1.5, FOCUS),
            egui::StrokeKind::Outside,
        );
    }
    response
}

pub fn timbre_selector(
    ui: &mut egui::Ui,
    selected: &mut usize,
    index: usize,
    enabled: bool,
) -> Response {
    let label = format!("{}  {}", index + 1, if enabled { "ON" } else { "OFF" });
    let response = ui.add(
        egui::Button::new(label)
            .selected(*selected == index)
            .fill(if *selected == index {
                Color32::from_rgb(62, 99, 128)
            } else {
                Color32::from_rgb(55, 60, 65)
            })
            .min_size(egui::vec2(64.0, 32.0)),
    );
    if response.clicked() {
        *selected = index;
    }
    response
}

#[derive(Clone, Copy, Debug)]
struct KnobPosition {
    precise: f32,
    published: u16,
}
impl KnobPosition {
    fn synchronize(&mut self, incoming: u16) {
        if incoming != self.published {
            self.precise = incoming as f32;
            self.published = incoming;
        }
    }
    fn move_by(&mut self, delta: f32, maximum: u16, step: u16) -> u16 {
        self.precise = (self.precise + delta).clamp(0.0, maximum as f32);
        self.published = ((self.precise / step as f32).round() as u16 * step).min(maximum);
        self.published
    }
    fn set(&mut self, value: u16) {
        self.precise = value as f32;
        self.published = value;
    }
}

pub struct KnobChange {
    pub response: Response,
    pub value: u16,
    pub changed: bool,
}

pub struct KnobSettings<'a> {
    pub incoming: u16,
    pub default: u16,
    pub native: bool,
    pub enabled: bool,
    pub label: &'a str,
    pub maximum: Option<u16>,
}

/// Vertical drag: 180 screen points traverses the range, Shift is ten times
/// finer. Fractional motion survives a native 7-bit parameter's quantization.
pub fn knob(
    ui: &mut egui::Ui,
    id: egui::Id,
    hit: egui::Rect,
    settings: KnobSettings<'_>,
) -> KnobChange {
    let KnobSettings {
        incoming,
        default,
        native,
        enabled,
        label,
        maximum: configured_maximum,
    } = settings;
    let step = if native { 8 } else { 1 };
    let maximum = configured_maximum.unwrap_or(if native { 1016 } else { 1023 });
    let mut response = ui.interact(
        hit,
        id,
        if enabled {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
    );
    let state_id = id.with("precise-position");
    let mut position = ui.ctx().data_mut(|data| {
        data.get_temp::<KnobPosition>(state_id)
            .unwrap_or(KnobPosition {
                precise: incoming as f32,
                published: incoming,
            })
    });
    position.synchronize(incoming);
    if enabled {
        if response.clicked() || response.drag_started() {
            response.request_focus();
        }
        let fine = ui.input(|i| i.modifiers.shift);
        let precision = if fine { 0.1 } else { 1.0 };
        let mut delta = 0.0;
        if response.dragged() || response.drag_stopped_by(egui::PointerButton::Primary) {
            let movement = ui.input(|i| i.pointer.delta());
            delta -= movement.y * maximum as f32 / 180.0 * precision;
        }
        // Consume the wheel delta so the containing panel does not move while
        // the user is editing a dial. Preserve magnitude instead of signum.
        if response.hovered() {
            delta += ui.input_mut(|i| {
                let delta = i.smooth_scroll_delta.y;
                i.smooth_scroll_delta.y = 0.0;
                delta
            }) * step as f32
                / 8.0
                * precision;
        }
        if response.has_focus() {
            delta += ui.input(|i| {
                let amount = if fine { step } else { step.max(8) } as f32;
                let up = i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowRight);
                let down =
                    i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowLeft);
                (i32::from(up) - i32::from(down)) as f32 * amount
            });
        }
        use egui::accesskit::{Action, ActionData};
        delta += ui.input(|i| {
            i.num_accesskit_action_requests(response.id, Action::Increment) as f32
                - i.num_accesskit_action_requests(response.id, Action::Decrement) as f32
        }) * step as f32;
        let requested = ui.input(|i| {
            i.accesskit_action_requests(response.id, Action::SetValue)
                .filter_map(|request| match request.data {
                    Some(ActionData::NumericValue(value)) if value.is_finite() => Some(value),
                    _ => None,
                })
                .last()
        });
        position.move_by(delta, maximum, step);
        if let Some(requested) = requested {
            let value = requested.clamp(0.0, (maximum / step) as f64).round() as u16 * step;
            position.set(value);
        }
        if response.double_clicked() {
            position.set(default.min(maximum));
        }
        response.context_menu(|ui| {
            let mut displayed = position.published / step;
            ui.horizontal(|ui| {
                ui.label(label);
                if ui
                    .add(egui::DragValue::new(&mut displayed).range(0..=maximum / step))
                    .changed()
                {
                    position.set(displayed * step);
                }
            });
            if ui.button("Сбросить").clicked() {
                position.set(default.min(maximum));
                ui.close();
            }
        });
        response = response
            .on_hover_cursor(egui::CursorIcon::ResizeVertical)
            .on_hover_text(format!(
                "{} · {}\nВверх / вниз · Shift: точно · Двойной клик: сброс",
                label,
                position.published / step
            ));
    } else {
        response = response
            .on_hover_cursor(egui::CursorIcon::NotAllowed)
            .on_hover_text(format!("{} · ещё не подключено к движку", label));
    }
    let value = position.published;
    let changed = enabled && value != incoming;
    if changed {
        response.mark_changed();
    }
    ui.ctx()
        .data_mut(|data| data.insert_temp(state_id, position));
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Slider, enabled, label);
        info.value = Some((value / step) as f64);
        info
    });
    if enabled {
        ui.ctx().accesskit_node_builder(response.id, |node| {
            use egui::accesskit::Action;
            node.set_numeric_value((value / step) as f64);
            node.set_min_numeric_value(0.0);
            node.set_max_numeric_value((maximum / step) as f64);
            node.set_numeric_value_step(1.0);
            node.add_action(Action::SetValue);
            node.add_action(Action::Increment);
            node.add_action(Action::Decrement);
        });
    }
    KnobChange {
        response,
        value,
        changed,
    }
}

pub struct SwitchStyle<'a> {
    pub enabled: bool,
    pub selected: bool,
    pub step: bool,
    pub label: Option<&'a str>,
}

pub fn switch_face(
    painter: &egui::Painter,
    face: egui::Rect,
    response: &Response,
    scale: f32,
    style: SwitchStyle<'_>,
) {
    let SwitchStyle {
        enabled,
        selected,
        step,
        label,
    } = style;
    let fill = if !enabled {
        Color32::from_rgb(160, 166, 172)
    } else if selected || response.is_pointer_button_down_on() {
        ACTIVE
    } else if step {
        Color32::from_rgb(180, 65, 76)
    } else {
        INK
    };
    painter.rect_filled(
        face.translate(Vec2::new(0.0, scale * 1.5)),
        3.0 * scale,
        Color32::from_gray(103),
    );
    painter.rect_filled(face, 3.0 * scale, fill);
    painter.rect_stroke(
        face,
        3.0 * scale,
        Stroke::new(
            scale,
            if enabled {
                INK
            } else {
                Color32::from_gray(102)
            },
        ),
        egui::StrokeKind::Inside,
    );
    if enabled && (response.hovered() || response.has_focus()) {
        painter.rect_stroke(
            face.expand(2.0 * scale),
            3.0 * scale,
            Stroke::new(1.5 * scale, Color32::from_rgb(62, 99, 128)),
            egui::StrokeKind::Outside,
        );
    }
    if let Some(label) = label {
        painter.text(
            face.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(9.0 * scale),
            if enabled {
                Color32::WHITE
            } else {
                Color32::from_rgb(91, 99, 107)
            },
        );
    }
    if !enabled {
        let center = face.right_top() + Vec2::new(-5.0, 5.0) * scale;
        let stroke = Stroke::new(scale, Color32::from_rgb(91, 99, 107));
        painter.line_segment(
            [
                center + Vec2::new(-2.0, -2.0) * scale,
                center + Vec2::new(2.0, 2.0) * scale,
            ],
            stroke,
        );
        painter.line_segment(
            [
                center + Vec2::new(-2.0, 2.0) * scale,
                center + Vec2::new(2.0, -2.0) * scale,
            ],
            stroke,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{KnobPosition, knob, note_key_active};
    use eframe::egui;

    fn frame(ctx: &egui::Context, parameter: &mut u16, events: Vec<egui::Event>) -> bool {
        frame_with_maximum(ctx, parameter, events, None)
    }
    fn frame_with_maximum(
        ctx: &egui::Context,
        parameter: &mut u16,
        events: Vec<egui::Event>,
        maximum: Option<u16>,
    ) -> bool {
        let mut dragged = false;
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 400.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("application")
                    .show(ui, |ui| {
                        egui::ScrollArea::both()
                            .id_salt("faceplate")
                            .show(ui, |ui| {
                                ui.allocate_exact_size(
                                    egui::vec2(400.0, 400.0),
                                    egui::Sense::hover(),
                                );
                                let result = knob(
                                    ui,
                                    ui.id().with("test-knob"),
                                    egui::Rect::from_center_size(
                                        egui::pos2(200.0, 200.0),
                                        egui::vec2(48.0, 48.0),
                                    ),
                                    super::KnobSettings {
                                        incoming: *parameter,
                                        default: 512,
                                        native: true,
                                        enabled: true,
                                        label: "Cutoff",
                                        maximum,
                                    },
                                );
                                *parameter = result.value;
                                dragged = result.response.dragged();
                            });
                    });
            },
        );
        output.drop_without_applying_deltas();
        dragged
    }

    fn pointer_button(pressed: bool, x: f32, y: f32) -> egui::Event {
        egui::Event::PointerButton {
            pos: egui::pos2(x, y),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    #[test]
    fn a_complete_note_click_between_frames_is_not_lost() {
        let ctx = egui::Context::default();
        let mut active = false;
        let mut old_pointer_only = false;
        let mut render = |events, previously_held| {
            let output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = ui.add(egui::Button::new("60").min_size(egui::vec2(64.0, 38.0)));
                    old_pointer_only = response.is_pointer_button_down_on();
                    active = note_key_active(&response, previously_held);
                },
            );
            output.drop_without_applying_deltas();
        };
        render(vec![], false);
        render(
            vec![
                egui::Event::PointerMoved(egui::pos2(16.0, 16.0)),
                pointer_button(true, 16.0, 16.0),
                pointer_button(false, 16.0, 16.0),
            ],
            false,
        );
        assert!(
            !old_pointer_only,
            "the previous pointer-only code lost this click"
        );
        assert!(active, "a complete click must still produce NoteOn");
        // No synthetic hold is added to the next frame.
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let response = ui.add(egui::Button::new("60").min_size(egui::vec2(64.0, 38.0)));
            active = note_key_active(&response, true);
        });
        output.drop_without_applying_deltas();
        assert!(!active);
    }

    #[test]
    fn releasing_a_previously_held_note_does_not_retrigger_it() {
        let ctx = egui::Context::default();
        let mut active = false;
        let mut render = |events, previously_held| {
            let output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = ui.add(egui::Button::new("60").min_size(egui::vec2(64.0, 38.0)));
                    active = note_key_active(&response, previously_held);
                },
            );
            output.drop_without_applying_deltas();
            active
        };
        render(vec![], false);
        assert!(render(
            vec![
                egui::Event::PointerMoved(egui::pos2(16.0, 16.0)),
                pointer_button(true, 16.0, 16.0)
            ],
            false
        ));
        assert!(!render(vec![pointer_button(false, 16.0, 16.0)], true));
    }

    #[test]
    fn tempo_division_drag_preserves_fractional_feedback() {
        let ctx = egui::Context::default();
        let mut parameter = 64;
        frame_with_maximum(&ctx, &mut parameter, vec![], Some(128));
        frame_with_maximum(
            &ctx,
            &mut parameter,
            vec![
                egui::Event::PointerMoved(egui::pos2(200.0, 200.0)),
                pointer_button(true, 200.0, 200.0),
            ],
            Some(128),
        );
        frame_with_maximum(
            &ctx,
            &mut parameter,
            vec![egui::Event::PointerMoved(egui::pos2(200.0, 188.0))],
            Some(128),
        );
        let before = parameter;
        for step in 1..=64 {
            frame_with_maximum(
                &ctx,
                &mut parameter,
                vec![egui::Event::PointerMoved(egui::pos2(
                    200.0,
                    188.0 - step as f32 * 0.25,
                ))],
                Some(128),
            );
            // The panel publishes the actual integer division on every frame.
            parameter = (parameter / 8) * 8;
        }
        frame_with_maximum(
            &ctx,
            &mut parameter,
            vec![pointer_button(false, 200.0, 172.0)],
            Some(128),
        );
        assert!(parameter > before);
        assert_eq!(parameter % 8, 0);
    }

    #[test]
    fn pointer_capture_continues_outside_dial_then_stops_on_release() {
        let ctx = egui::Context::default();
        let mut parameter = 512;
        frame(&ctx, &mut parameter, vec![]);
        frame(
            &ctx,
            &mut parameter,
            vec![
                egui::Event::PointerMoved(egui::pos2(200.0, 200.0)),
                pointer_button(true, 200.0, 200.0),
            ],
        );
        assert_eq!(parameter, 512, "pressing the knob must not jump its value");
        assert!(frame(
            &ctx,
            &mut parameter,
            vec![egui::Event::PointerMoved(egui::pos2(200.0, 192.0))]
        ));
        assert_eq!(parameter, 560);
        assert!(frame(
            &ctx,
            &mut parameter,
            vec![egui::Event::PointerMoved(egui::pos2(200.0, 140.0))]
        ));
        assert_eq!(parameter, 848);
        frame(
            &ctx,
            &mut parameter,
            vec![pointer_button(false, 200.0, 140.0)],
        );
        assert!(!frame(
            &ctx,
            &mut parameter,
            vec![egui::Event::PointerMoved(egui::pos2(200.0, 100.0))]
        ));
        assert_eq!(
            parameter, 848,
            "released movement must not change the parameter"
        );
    }

    #[test]
    fn native_fractional_drag_survives_parameter_feedback() {
        let mut value = KnobPosition {
            precise: 512.0,
            published: 512,
        };
        let mut parameter = 512;
        for _ in 0..80 {
            value.synchronize(parameter);
            parameter = value.move_by(0.1, 1016, 8);
        }
        assert_eq!(parameter, 520);
        assert!((value.precise - 520.0).abs() < 0.01);
    }

    #[test]
    fn wheel_magnitude_is_consumed_by_dial_inside_nested_scroll_areas() {
        let ctx = egui::Context::default();
        let mut parameter = 512;
        frame(
            &ctx,
            &mut parameter,
            vec![egui::Event::PointerMoved(egui::pos2(200.0, 200.0))],
        );
        frame(
            &ctx,
            &mut parameter,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 32.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
            }],
        );
        for _ in 0..32 {
            frame(&ctx, &mut parameter, vec![]);
        }
        assert_eq!(parameter, 544);
        assert_eq!(ctx.input(|i| i.smooth_scroll_delta.y), 0.0);
    }

    #[test]
    fn saturation_has_no_hidden_overshoot_when_drag_reverses() {
        let mut value = KnobPosition {
            precise: 1016.0,
            published: 1016,
        };
        assert_eq!(value.move_by(300.0, 1016, 8), 1016);
        assert_eq!(value.move_by(-8.0, 1016, 8), 1008);
        value.synchronize(256);
        assert_eq!(value.move_by(0.0, 1016, 8), 256);
    }
}
