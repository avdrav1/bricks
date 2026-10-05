// DEC-1 spike: custom-painted virtual grid in egui/eframe (wgpu). Throwaway.
#[path = "../../common.rs"]
mod common;

use common::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Sense};
use eframe::egui_wgpu;
use std::fmt::Write as _;

const SB_W: f32 = 14.0;

struct Grid {
    args: Args,
    scroll: f64,
    frame: u32,
    ready: bool,
    buf: String,
}

/// Paint callback appended last; its `paint` runs while egui records the frame's render pass.
struct FrameMark {
    top_row: u64,
    cpu_us: u64,
}

impl egui_wgpu::CallbackTrait for FrameMark {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        _render_pass: &mut egui_wgpu::wgpu::RenderPass<'static>,
        _resources: &egui_wgpu::CallbackResources,
    ) {
        emit(format_args!(
            "F {} {} {}",
            mono_ns(),
            self.top_row,
            self.cpu_us
        ));
    }
}

impl eframe::App for Grid {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        for e in &raw.events {
            if let egui::Event::Key {
                key, pressed: true, ..
            } = e
            {
                emit(format_args!("K {} {}", mono_ns(), key.name()));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let args = self.args;
        let cpu_us = frame.info().cpu_usage.map_or(0, |s| (s * 1e6) as u64);
        let full = ui.max_rect();
        let grid_rect = Rect::from_min_max(full.min, pos2(full.max.x - SB_W, full.max.y));
        let sb_rect = Rect::from_min_max(
            pos2(full.max.x - SB_W, full.min.y + HEADER_H as f32),
            full.max,
        );
        let body_h = grid_rect.height() as f64 - HEADER_H;
        let max_scroll = (args.content_height() - body_h).max(0.0);

        // Input.
        ui.input(|i| {
            for e in &i.events {
                if let egui::Event::Key {
                    key, pressed: true, ..
                } = e
                {
                    self.scroll = match key {
                        egui::Key::PageDown => self.scroll + body_h * 0.9,
                        egui::Key::PageUp => self.scroll - body_h * 0.9,
                        egui::Key::ArrowDown => self.scroll + ROW_H,
                        egui::Key::ArrowUp => self.scroll - ROW_H,
                        egui::Key::Home => 0.0,
                        egui::Key::End => max_scroll,
                        _ => self.scroll,
                    };
                }
            }
            self.scroll -= i.smooth_scroll_delta.y as f64;
        });

        // Scrollbar: absolute mapping, thumb centre follows the pointer.
        let sb = ui.interact(sb_rect, ui.id().with("vscroll"), Sense::click_and_drag());
        if sb.dragged() || sb.clicked() {
            if let Some(p) = sb.interact_pointer_pos() {
                let frac = ((p.y - sb_rect.min.y) / sb_rect.height()).clamp(0.0, 1.0) as f64;
                self.scroll = frac * max_scroll;
            }
        }

        if args.mode != Mode::Interactive {
            self.frame += 1;
            if self.frame > args.frames {
                emit(format_args!("DONE"));
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.scroll = autopilot(args.mode, self.frame, self.scroll, body_h, args.rows);
            }
            ui.ctx().request_repaint();
        }
        self.scroll = clamp_scroll(self.scroll, body_h, args.rows);

        // Paint.
        let vp = viewport(self.scroll, body_h, args.rows);
        let ncols = ((((grid_rect.width() as f64) - ROW_HEADER_W) / COL_W)
            .ceil()
            .max(0.0) as u32)
            .min(args.cols);
        if !self.ready {
            self.ready = true;
            emit(format_args!(
                "READY {} {} {} {}",
                grid_rect.width(),
                grid_rect.height(),
                vp.row_count,
                ncols
            ));
        }

        let painter = ui.painter_at(full);
        let o = grid_rect.min;
        let r = |x: f64, y: f64, w: f64, h: f64| {
            Rect::from_min_size(
                pos2(o.x + x as f32, o.y + y as f32),
                vec2(w as f32, h as f32),
            )
        };
        let w = grid_rect.width() as f64;
        let stripe = Color32::from_rgb(245, 247, 252);
        let line = Color32::from_rgb(217, 219, 224);
        let header_bg = Color32::from_rgb(235, 237, 242);
        let ink = Color32::from_rgb(26, 26, 30);
        let font = FontId::proportional(13.0);

        painter.rect_filled(full, 0.0, Color32::WHITE);

        for i in 0..vp.row_count {
            let y = HEADER_H + vp.y_offset + i as f64 * ROW_H;
            if (vp.first_row + i) % 2 == 1 {
                painter.rect_filled(r(0.0, y, w, ROW_H), 0.0, stripe);
            }
            painter.rect_filled(r(0.0, y + ROW_H - 1.0, w, 1.0), 0.0, line);
        }

        for c in 0..ncols {
            let x = ROW_HEADER_W + c as f64 * COL_W;
            painter.rect_filled(r(x + COL_W - 1.0, HEADER_H, 1.0, body_h), 0.0, line);
            let p = painter.with_clip_rect(r(x, HEADER_H, COL_W - 1.0, body_h));
            for i in 0..vp.row_count {
                let y = HEADER_H + vp.y_offset + i as f64 * ROW_H;
                cell_text(vp.first_row + i, c, &mut self.buf);
                p.text(
                    r(x + 4.0, y + 3.0, 0.0, 0.0).min,
                    Align2::LEFT_TOP,
                    &self.buf,
                    font.clone(),
                    ink,
                );
            }
        }

        painter.rect_filled(r(0.0, HEADER_H, ROW_HEADER_W, body_h), 0.0, header_bg);
        let p = painter.with_clip_rect(r(0.0, HEADER_H, ROW_HEADER_W, body_h));
        for i in 0..vp.row_count {
            let y = HEADER_H + vp.y_offset + i as f64 * ROW_H;
            self.buf.clear();
            let _ = write!(self.buf, "{}", vp.first_row + i + 1);
            p.text(
                r(6.0, y + 3.0, 0.0, 0.0).min,
                Align2::LEFT_TOP,
                &self.buf,
                font.clone(),
                ink,
            );
        }

        painter.rect_filled(r(0.0, 0.0, w, HEADER_H), 0.0, header_bg);
        for c in 0..ncols {
            let x = ROW_HEADER_W + c as f64 * COL_W;
            col_name(c, &mut self.buf);
            painter.text(
                r(x + COL_W / 2.0, 4.0, 0.0, 0.0).min,
                Align2::CENTER_TOP,
                &self.buf,
                font.clone(),
                ink,
            );
        }

        // Scrollbar visuals.
        painter.rect_filled(sb_rect, 0.0, header_bg);
        let thumb_h = (sb_rect.height() as f64 * body_h / args.content_height()).max(24.0) as f32;
        let frac = if max_scroll > 0.0 {
            (self.scroll / max_scroll) as f32
        } else {
            0.0
        };
        let thumb_y = sb_rect.min.y + frac * (sb_rect.height() - thumb_h);
        painter.rect_filled(
            Rect::from_min_size(
                pos2(sb_rect.min.x + 3.0, thumb_y),
                vec2(SB_W - 6.0, thumb_h),
            ),
            4.0,
            Color32::from_gray(150),
        );

        painter.add(egui_wgpu::Callback::new_paint_callback(
            Rect::from_min_size(full.min, vec2(1.0, 1.0)),
            FrameMark {
                top_row: vp.first_row,
                cpu_us,
            },
        ));
    }
}

fn main() -> eframe::Result {
    let args = Args::parse();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("dec1-egui")
            .with_app_id("dev.bricks.Dec1Egui")
            .with_inner_size([1600.0, 1000.0]),
        ..Default::default()
    };
    eframe::run_native(
        "dec1-egui",
        options,
        Box::new(move |cc| {
            if std::env::var_os("DEC1_UNICODE_FONTS").is_some() {
                add_fallback_fonts(&cc.egui_ctx);
            }
            Ok(Box::new(Grid {
                args,
                scroll: 0.0,
                frame: 0,
                ready: false,
                buf: String::new(),
            }))
        }),
    )
}

/// Text-correctness probe: egui ships no system font fallback, so append Noto fonts by hand.
fn add_fallback_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for path in [
        "/usr/share/fonts/noto/NotoNaskhArabic-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansHebrew-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansDevanagari-Regular.ttf",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ] {
        let bytes = std::fs::read(path).expect(path);
        fonts.font_data.insert(
            path.to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .push(path.to_owned());
    }
    ctx.set_fonts(fonts);
}
