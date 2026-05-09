mod engraver;
 
use engraver::Engraver;
use engraver::protocol::{MAX_WIDTH, MAX_HEIGHT};
 
use anyhow::Result;
use eframe::{egui, NativeOptions};
use egui::{
    Color32, RichText, Stroke, Vec2, Rect, Pos2, Rounding,
    ColorImage, TextureHandle, TextureOptions,
};
use std::sync::{Arc, Mutex};
 
// ── app state ─────────────────────────────────────────────────────────────────
 
struct App {
    // connection
    ports:          Vec<String>,
    selected_port:  usize,
    engraver:       Option<Arc<Mutex<Engraver>>>,
    connect_error:  Option<String>,
 
    // image
    image_path:       Option<String>,
    loaded_image:     Option<::image::DynamicImage>,
    preview_tex:      Option<TextureHandle>,
    /// 1-bit thresholded preview, regenerated when threshold changes
    threshold_tex:    Option<TextureHandle>,
    /// the threshold value used to build threshold_tex (detect staleness)
    threshold_tex_at: u8,
 
    // burn settings
    origin_x:   u16,
    origin_y:   u16,
    power:      u8,
    idle_power: u8,
    threshold:  u8,
 
    // jog
    jog_step: i32,
 
    // status
    status_msg: String,
    log:        Vec<String>,
}
 
impl Default for App {
    fn default() -> Self {
        let ports = Engraver::list_ports().unwrap_or_default();
        Self {
            ports,
            selected_port: 0,
            engraver: None,
            connect_error: None,
            image_path: None,
            loaded_image: None,
            preview_tex: None,
            threshold_tex: None,
            threshold_tex_at: 255, // sentinel: force rebuild on first frame
            origin_x: MAX_WIDTH  / 2,
            origin_y: MAX_HEIGHT / 2,
            power: 80,
            idle_power: 10,
            threshold: 128,
            jog_step: 10,
            status_msg: String::from("Disconnected"),
            log: Vec::new(),
        }
    }
}
 
impl App {
    fn log(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        self.status_msg = m.clone();
        self.log.push(m);
        if self.log.len() > 200 { self.log.remove(0); }
    }
 
    fn with_engraver<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Engraver) -> Result<()>,
    {
        if let Some(arc) = self.engraver.clone() {
            let mut e = arc.lock().unwrap();
            if let Err(err) = f(&mut e) {
                drop(e); // release lock before mutable borrow of self
                self.log(format!("Error: {err}"));
            }
        } else {
            self.log("Not connected");
        }
    }
 
    fn connect(&mut self) {
        let port = match self.ports.get(self.selected_port) {
            Some(p) => p.clone(),
            None    => { self.log("No port selected"); return; }
        };
        match Engraver::connect(&port) {
            Ok(e)  => {
                self.engraver = Some(Arc::new(Mutex::new(e)));
                self.connect_error = None;
                self.log(format!("Connected to {port}"));
            }
            Err(e) => {
                let msg = format!("Connect failed: {e}");
                self.connect_error = Some(msg.clone());
                self.log(msg);
            }
        }
    }
 
    fn disconnect(&mut self) {
        self.engraver = None;
        self.log("Disconnected");
    }
 
    /// Rebuild the thresholded preview texture from the loaded image.
    /// Call this whenever `threshold` changes or a new image is loaded.
    fn rebuild_threshold_tex(&mut self, ctx: &egui::Context) {
        if let Some(img) = &self.loaded_image {
            let thr = self.threshold;
            let thresholded = engraver::image::threshold_image(img, thr);
            let rgba = thresholded.to_rgba8();
            let (w, h) = (rgba.width() as usize, rgba.height() as usize);
            let ci = ColorImage::from_rgba_unmultiplied([w, h], rgba.as_raw());
            self.threshold_tex    = Some(ctx.load_texture("threshold_preview", ci, TextureOptions::default()));
            self.threshold_tex_at = thr;
        }
    }
 
    fn load_image_dialog(&mut self, ctx: &egui::Context) {
        let path = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "tiff", "webp"])
            .pick_file();
 
        if let Some(p) = path {
            let path_str = p.to_string_lossy().to_string();
            match ::image::open(&p) {
                Ok(img) => {
                    // build egui texture for preview
                    let rgba = img.to_rgba8();
                    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
                    let ci = ColorImage::from_rgba_unmultiplied([w, h], rgba.as_raw());
                    self.preview_tex = Some(ctx.load_texture(
                        "preview",
                        ci,
                        TextureOptions::default(),
                    ));
                    self.loaded_image   = Some(img);
                    self.image_path     = Some(path_str.clone());
                    self.threshold_tex  = None;   // will rebuild next frame
                    self.threshold_tex_at = 255;  // sentinel
                    self.log(format!("Loaded: {path_str}"));
                }
                Err(e) => self.log(format!("Image load error: {e}")),
            }
        }
    }
}
 
// ── egui app impl ─────────────────────────────────────────────────────────────
 
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // dark industrial theme
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill           = Color32::from_rgb(18, 18, 22);
        visuals.window_fill          = Color32::from_rgb(26, 26, 32);
        visuals.extreme_bg_color     = Color32::from_rgb(12, 12, 16);
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(30, 30, 38);
        visuals.widgets.inactive.bg_fill       = Color32::from_rgb(40, 40, 50);
        visuals.widgets.hovered.bg_fill        = Color32::from_rgb(55, 55, 70);
        visuals.widgets.active.bg_fill         = Color32::from_rgb(220, 120, 20);
        visuals.selection.bg_fill              = Color32::from_rgb(200, 100, 10);
        ctx.set_visuals(visuals);
 
        // request continuous repaint while burning (for progress updates)
        if self.engraver.as_ref().map(|e| e.lock().unwrap().state().burning).unwrap_or(false) {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
 
        // rebuild threshold preview if slider moved or image just loaded
        if self.loaded_image.is_some() && self.threshold != self.threshold_tex_at {
            self.rebuild_threshold_tex(ctx);
        }
 
        egui::CentralPanel::default().show(ctx, |ui| {
            // ── title bar ────────────────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("⚡ CRUSTGRAVER")
                        .size(22.0)
                        .color(Color32::from_rgb(220, 120, 20))
                        .strong()
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(&self.status_msg)
                            .size(11.0)
                            .color(Color32::from_rgb(150, 150, 170))
                    );
                });
            });
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(6.0);
 
            // ── main columns ─────────────────────────────────────────────────
            ui.columns(2, |cols| {
                // ══ LEFT COLUMN ══════════════════════════════════════════════
                let ui = &mut cols[0];
 
                // ── connection panel ─────────────────────────────────────────
                section_header(ui, "CONNECTION");
                egui::Frame::none()
                    .fill(Color32::from_rgb(28, 28, 36))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_source("port_combo")
                                .selected_text(
                                    self.ports.get(self.selected_port)
                                        .map(|s| s.as_str())
                                        .unwrap_or("(none)")
                                )
                                .width(160.0)
                                .show_ui(ui, |ui| {
                                    for (i, p) in self.ports.iter().enumerate() {
                                        ui.selectable_value(&mut self.selected_port, i, p);
                                    }
                                });
 
                            if ui.button("↺").on_hover_text("Refresh ports").clicked() {
                                self.ports = Engraver::list_ports().unwrap_or_default();
                            }
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let connected = self.engraver.is_some();
                            if !connected {
                                if ui.add(
                                    egui::Button::new(
                                        RichText::new("Connect").color(Color32::WHITE)
                                    ).fill(Color32::from_rgb(30, 120, 60))
                                ).clicked() {
                                    self.connect();
                                }
                            } else {
                                if ui.add(
                                    egui::Button::new(
                                        RichText::new("Disconnect").color(Color32::WHITE)
                                    ).fill(Color32::from_rgb(140, 40, 40))
                                ).clicked() {
                                    self.disconnect();
                                }
                            }
 
                            let dot_col = if connected {
                                Color32::from_rgb(60, 200, 80)
                            } else {
                                Color32::from_rgb(160, 50, 50)
                            };
                            let label = if connected { "● Online" } else { "● Offline" };
                            ui.label(RichText::new(label).color(dot_col).size(12.0));
                        });
                        if let Some(e) = &self.connect_error {
                            ui.label(RichText::new(e).color(Color32::from_rgb(220, 80, 80)).size(11.0));
                        }
                    });
 
                ui.add_space(10.0);
 
                // ── device status ─────────────────────────────────────────────
                section_header(ui, "DEVICE STATUS");
                egui::Frame::none()
                    .fill(Color32::from_rgb(28, 28, 36))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        if let Some(arc) = &self.engraver {
                            let state = arc.lock().unwrap().state();
 
                            // temperature gauge
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("🌡 Temp:").size(13.0));
                                match state.temperature {
                                    None    => ui.label(
                                        RichText::new("—").color(Color32::from_rgb(120,120,140))
                                    ),
                                    Some(t) => {
                                        let col = temp_color(t);
                                        ui.label(RichText::new(format!("{t}°C")).color(col).strong())
                                    }
                                };
                            });
 
                            // position
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("📍 Pos:").size(13.0));
                                ui.label(RichText::new(
                                    format!("X {} / Y {}", state.position[0], state.position[1])
                                ).monospace());
                            });
 
                            // progress bar
                            if state.burning || state.progress < 100 {
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label("Progress:");
                                    let pct = state.progress as f32 / 100.0;
                                    ui.add(
                                        egui::ProgressBar::new(pct)
                                            .text(format!("{}%", state.progress))
                                            .desired_width(150.0)
                                    );
                                });
                            }
                        } else {
                            ui.label(
                                RichText::new("No device connected")
                                    .color(Color32::from_rgb(100, 100, 120))
                                    .italics()
                            );
                        }
                    });
 
                ui.add_space(10.0);
 
                // ── laser settings ────────────────────────────────────────────
                section_header(ui, "LASER SETTINGS");
                egui::Frame::none()
                    .fill(Color32::from_rgb(28, 28, 36))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Power (0–255):");
                            ui.add(egui::Slider::new(&mut self.power, 0..=255).clamp_to_range(true));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Idle  (0–255):");
                            ui.add(egui::Slider::new(&mut self.idle_power, 0..=255).clamp_to_range(true));
                        });
                        if ui.add(
                            egui::Button::new("Apply Power")
                                .fill(Color32::from_rgb(50, 50, 70))
                        ).clicked() {
                            let (p, i) = (self.power, self.idle_power);
                            self.with_engraver(|e| e.set_power(p, i));
                            self.log(format!("Power set: burn={p} idle={i}"));
                        }
                    });
 
                ui.add_space(10.0);
 
                // ── burn controls ─────────────────────────────────────────────
                section_header(ui, "BURN CONTROLS");
                egui::Frame::none()
                    .fill(Color32::from_rgb(28, 28, 36))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Origin X:");
                            ui.add(egui::DragValue::new(&mut self.origin_x).clamp_range(0..=MAX_WIDTH));
                            ui.label("Y:");
                            ui.add(egui::DragValue::new(&mut self.origin_y).clamp_range(0..=MAX_HEIGHT));
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            // preview button
                            if ui.add(
                                egui::Button::new("▷ Preview")
                                    .fill(Color32::from_rgb(40, 80, 140))
                            ).clicked() {
                                let (x, y) = (self.origin_x, self.origin_y);
                                let (w, h) = if let Some(img) = &self.loaded_image {
                                    (img.width() as u16, img.height() as u16)
                                } else {
                                    (100, 100)
                                };
                                self.with_engraver(|e| e.show_preview(x, y, w, h));
                                self.log("Preview started");
                            }
 
                            if ui.add(
                                egui::Button::new("◻ Stop Preview")
                                    .fill(Color32::from_rgb(60, 60, 80))
                            ).clicked() {
                                self.with_engraver(|e| e.stop_preview());
                                self.log("Preview stopped");
                            }
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let can_burn = self.engraver.is_some() && self.loaded_image.is_some();
                            let burn_btn = ui.add_enabled(
                                can_burn,
                                egui::Button::new(
                                    RichText::new("🔥 BURN").strong().color(Color32::WHITE)
                                ).fill(Color32::from_rgb(200, 70, 20))
                                 .min_size(Vec2::new(100.0, 30.0))
                            );
                            if burn_btn.clicked() {
                                let img   = self.loaded_image.clone().unwrap();
                                let x     = self.origin_x;
                                let y     = self.origin_y;
                                let thr   = self.threshold;
                                self.with_engraver(|e| e.burn_dynamic_image(&img, x, y, thr));
                                self.log("Burning started...");
                            }
                            if !can_burn {
                                ui.label(
                                    RichText::new(if self.engraver.is_none() {
                                        "connect first"
                                    } else {
                                        "load image first"
                                    }).color(Color32::from_rgb(120,120,140)).size(11.0)
                                );
                            }
 
                            if ui.add(
                                egui::Button::new("⏸ Pause")
                                    .fill(Color32::from_rgb(60, 90, 40))
                            ).clicked() {
                                self.with_engraver(|e| e.pause());
                            }
 
                            if ui.add(
                                egui::Button::new("⏹ Stop")
                                    .fill(Color32::from_rgb(140, 40, 40))
                            ).clicked() {
                                self.with_engraver(|e| e.stop());
                                self.log("Stopped");
                            }
                        });
                    });
            });
 
            // ══ RIGHT COLUMN is inside the columns closure but we need ctx ══
            // We'll draw right column items after the columns call using a Side Panel
        });
 
        // ── RIGHT SIDE PANEL ─────────────────────────────────────────────────
        egui::SidePanel::right("right_panel")
            .min_width(260.0)
            .max_width(400.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.add_space(8.0);
 
                // ── image picker ──────────────────────────────────────────────
                section_header(ui, "IMAGE");
                egui::Frame::none()
                    .fill(Color32::from_rgb(28, 28, 36))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.add(
                                egui::Button::new("📂 Open Image…")
                                    .fill(Color32::from_rgb(50, 50, 70))
                                    .min_size(Vec2::new(140.0, 26.0))
                            ).clicked() {
                                self.load_image_dialog(ctx);
                            }
                            if let Some(path) = &self.image_path {
                                let fname = std::path::Path::new(path)
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or(path.as_str());
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(fname).size(11.0).color(Color32::from_rgb(180,180,200)));
                                    if let Some(img) = &self.loaded_image {
                                        ui.label(
                                            RichText::new(format!("{}×{} px", img.width(), img.height()))
                                                .size(10.0)
                                                .color(Color32::from_rgb(120,120,140))
                                        );
                                    }
                                });
                            }
                        });
 
                        ui.add_space(6.0);
 
                        // ── threshold slider ──────────────────────────────────
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Threshold:").size(12.0));
                            let resp = ui.add(
                                egui::Slider::new(&mut self.threshold, 0..=255)
                                    .clamp_to_range(true)
                                    .show_value(true)
                            );
                            if resp.on_hover_text(
                                "Pixels darker than this value will be burned by the laser.\n\
                                 Lower = fewer pixels burned (only darkest).\n\
                                 Higher = more pixels burned (even light grays).\n\
                                 Try 64–100 for logos, 150–180 for photos."
                            ).changed() {
                                // texture will auto-rebuild at top of next update()
                            }
                        });
 
                        // burn-pixels-% indicator
                        if let Some(img) = &self.loaded_image {
                            let thr = self.threshold;
                            let total = (img.width() * img.height()) as u64;
                            if total > 0 {
                                // sample every 4th pixel for speed
                                let gray = img.to_luma8();
                                let dark = gray.pixels()
                                    .step_by(4)
                                    .filter(|p| p[0] < thr)
                                    .count() as u64 * 4;
                                let pct = (dark * 100 / total).min(100);
                                let col = match pct {
                                    0..=20  => Color32::from_rgb(80, 180, 100),
                                    21..=60 => Color32::from_rgb(200, 170, 50),
                                    _       => Color32::from_rgb(210, 80, 60),
                                };
                                ui.label(
                                    RichText::new(format!("~{pct}% of pixels will be burned"))
                                        .size(10.0)
                                        .color(col)
                                );
                            }
                        }
 
                        ui.add_space(6.0);
 
                        // ── side-by-side preview ──────────────────────────────
                        if self.preview_tex.is_some() || self.threshold_tex.is_some() {
                            let avail_w = ui.available_width();
                            let half_w  = (avail_w / 2.0 - 6.0).max(60.0);
 
                            ui.horizontal(|ui| {
                                // original
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new("Original")
                                            .size(10.0)
                                            .color(Color32::from_rgb(140, 140, 160))
                                    );
                                    if let Some(tex) = &self.preview_tex {
                                        let ar = tex.size()[1] as f32 / tex.size()[0] as f32;
                                        ui.add(
                                            egui::Image::new(tex)
                                                .fit_to_exact_size(Vec2::new(half_w, half_w * ar))
                                        );
                                    }
                                });
 
                                ui.add_space(4.0);
 
                                // thresholded
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new("Will burn  (black = laser on)")
                                            .size(10.0)
                                            .color(Color32::from_rgb(220, 140, 60))
                                    );
                                    if let Some(tex) = &self.threshold_tex {
                                        let ar = tex.size()[1] as f32 / tex.size()[0] as f32;
                                        ui.add(
                                            egui::Image::new(tex)
                                                .fit_to_exact_size(Vec2::new(half_w, half_w * ar))
                                        );
                                    } else {
                                        let ph = Rect::from_min_size(
                                            ui.cursor().min,
                                            Vec2::new(half_w, half_w),
                                        );
                                        ui.allocate_rect(ph, egui::Sense::hover());
                                        ui.painter().rect_filled(ph, Rounding::same(3.0), Color32::from_rgb(14, 14, 20));
                                        ui.painter().text(
                                            ph.center(),
                                            egui::Align2::CENTER_CENTER,
                                            "…",
                                            egui::FontId::proportional(14.0),
                                            Color32::from_rgb(80, 80, 100),
                                        );
                                    }
                                });
                            });
                        } else {
                            // no image yet — placeholder
                            let ph_h = 100.0;
                            let (r, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), ph_h),
                                egui::Sense::hover()
                            );
                            ui.painter().rect_filled(r, Rounding::same(4.0), Color32::from_rgb(20, 20, 28));
                            ui.painter().text(
                                r.center(),
                                egui::Align2::CENTER_CENTER,
                                "No image loaded",
                                egui::FontId::proportional(13.0),
                                Color32::from_rgb(80, 80, 100),
                            );
                        }
                    });
 
                ui.add_space(10.0);
 
                // ── jog panel ─────────────────────────────────────────────────
                section_header(ui, "HEAD CONTROL (JOG)");
                egui::Frame::none()
                    .fill(Color32::from_rgb(28, 28, 36))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Step:");
                            ui.add(
                                egui::DragValue::new(&mut self.jog_step)
                                    .clamp_range(1..=100)
                                    .suffix(" px")
                            );
                        });
 
                        ui.add_space(4.0);
 
                        // D-pad layout
                        let step = self.jog_step;
                        let btn_size = Vec2::new(44.0, 32.0);
                        let btn_fill = Color32::from_rgb(44, 44, 58);
                        let btn_fill_home = Color32::from_rgb(38, 60, 90);
 
                        ui.vertical_centered(|ui| {
                            // ▲ row
                            if ui.add(
                                egui::Button::new("▲").fill(btn_fill).min_size(btn_size)
                            ).clicked() {
                                self.with_engraver(|e| e.jog(0, -(step as i32)));
                            }
 
                            // ◄ HOME ► row
                            ui.horizontal(|ui| {
                                if ui.add(
                                    egui::Button::new("◄").fill(btn_fill).min_size(btn_size)
                                ).clicked() {
                                    self.with_engraver(|e| e.jog(-(step as i32), 0));
                                }
                                if ui.add(
                                    egui::Button::new("⌂").fill(btn_fill_home).min_size(btn_size)
                                ).on_hover_text("Move to centre").clicked() {
                                    let cx = MAX_WIDTH  / 2;
                                    let cy = MAX_HEIGHT / 2;
                                    self.with_engraver(|e| e.move_to(cx, cy));
                                    self.log("Moved to centre");
                                }
                                if ui.add(
                                    egui::Button::new("►").fill(btn_fill).min_size(btn_size)
                                ).clicked() {
                                    self.with_engraver(|e| e.jog(step as i32, 0));
                                }
                            });
 
                            // ▼ row
                            if ui.add(
                                egui::Button::new("▼").fill(btn_fill).min_size(btn_size)
                            ).clicked() {
                                self.with_engraver(|e| e.jog(0, step as i32));
                            }
                        });
 
                        ui.add_space(6.0);
                        // mini work-area map
                        let map_size = Vec2::new(ui.available_width(), 100.0);
                        let (rect, _) = ui.allocate_exact_size(map_size, egui::Sense::hover());
                        draw_workarea_map(ui, rect, &self.engraver);
                    });
 
                ui.add_space(10.0);
 
                // ── log ───────────────────────────────────────────────────────
                section_header(ui, "LOG");
                egui::ScrollArea::vertical()
                    .max_height(140.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &self.log {
                            ui.label(
                                RichText::new(line).size(11.0).monospace()
                                    .color(Color32::from_rgb(160, 200, 160))
                            );
                        }
                    });
            });
    }
}
 
// ── helpers ───────────────────────────────────────────────────────────────────
 
fn section_header(ui: &mut egui::Ui, title: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .size(10.0)
                .color(Color32::from_rgb(160, 120, 60))
                .strong()
        );
        ui.add(egui::Separator::default().horizontal().spacing(4.0));
    });
    ui.add_space(4.0);
}
 
fn temp_color(t: u8) -> Color32 {
    match t {
        0..=40  => Color32::from_rgb(80, 200, 120),
        41..=55 => Color32::from_rgb(230, 180, 40),
        _       => Color32::from_rgb(220, 60, 60),
    }
}
 
/// Draw a tiny overhead map of the work area with the current head position.
fn draw_workarea_map(
    ui: &egui::Ui,
    rect: Rect,
    engraver: &Option<Arc<Mutex<Engraver>>>,
) {
    let painter = ui.painter();
 
    // background
    painter.rect_filled(rect, Rounding::same(4.0), Color32::from_rgb(14, 14, 20));
    painter.rect_stroke(rect, Rounding::same(4.0), Stroke::new(1.0, Color32::from_rgb(60, 60, 80)));
 
    // crosshair grid lines
    let cx = rect.center();
    let dim_gray = Color32::from_rgb(35, 35, 50);
    painter.line_segment([Pos2::new(cx.x, rect.top()), Pos2::new(cx.x, rect.bottom())],
                         Stroke::new(0.5, dim_gray));
    painter.line_segment([Pos2::new(rect.left(), cx.y), Pos2::new(rect.right(), cx.y)],
                         Stroke::new(0.5, dim_gray));
 
    // head position dot
    if let Some(arc) = engraver {
        let state = arc.lock().unwrap().state();
        let nx = state.position[0] as f32 / MAX_WIDTH  as f32;
        let ny = state.position[1] as f32 / MAX_HEIGHT as f32;
        let px = rect.left() + nx * rect.width();
        let py = rect.top()  + ny * rect.height();
 
        // crosshair
        let ch = 6.0;
        let orange = Color32::from_rgb(220, 120, 20);
        painter.line_segment([Pos2::new(px - ch, py), Pos2::new(px + ch, py)],
                             Stroke::new(1.5, orange));
        painter.line_segment([Pos2::new(px, py - ch), Pos2::new(px, py + ch)],
                             Stroke::new(1.5, orange));
        painter.circle_stroke(Pos2::new(px, py), 3.5, Stroke::new(1.0, orange));
    }
 
    // label
    ui.painter().text(
        Pos2::new(rect.left() + 4.0, rect.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "work area",
        egui::FontId::proportional(9.0),
        Color32::from_rgb(80, 80, 100),
    );
}
 
// ── entry point ───────────────────────────────────────────────────────────────
 
fn main() -> Result<()> {
    env_logger::init();
 
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("CrustGraver — NEJE KZ 3000")
            .with_inner_size([900.0, 620.0])
            .with_min_inner_size([700.0, 480.0]),
        ..Default::default()
    };
 
    eframe::run_native(
        "CrustGraver",
        options,
        Box::new(|_cc| Box::new(App::default())),
    ).map_err(|e| anyhow::anyhow!("GUI error: {e}"))
}

