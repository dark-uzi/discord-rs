use eframe::egui::{self, RichText};

use crate::format::{split_emoji, twemoji_url, EmojiPiece};

fn status_color(status: &str) -> egui::Color32 {
    match status {
        "online" => egui::Color32::from_rgb(59, 165, 93),
        "idle" => egui::Color32::from_rgb(250, 166, 26),
        "dnd" => egui::Color32::from_rgb(237, 66, 69),
        _ => egui::Color32::from_rgb(128, 132, 142),
    }
}

pub(crate) fn presence_rank(status: &str) -> u8 {
    match status {
        "online" => 0,
        "idle" => 1,
        "dnd" => 2,
        _ => 3,
    }
}

/// Draw a circular avatar with a presence ring around it (Discord-style).
/// Returns the clickable response covering the avatar + ring.
pub(crate) fn avatar_ring(
    ui: &mut egui::Ui,
    url: &str,
    size: f32,
    status: Option<&str>,
) -> egui::Response {
    let ring = (size * 0.09).clamp(2.0, 4.0);
    let total = size + ring * 2.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(total, total), egui::Sense::click());
    let center = rect.center();
    if let Some(st) = status {
        let color = if st == "offline" || st.is_empty() {
            egui::Color32::from_gray(55)
        } else {
            status_color(st)
        };
        ui.painter()
            .circle_stroke(center, size / 2.0 + ring * 0.6, egui::Stroke::new(ring, color));
    }
    let avatar_rect = egui::Rect::from_center_size(center, egui::vec2(size, size));
    egui::Image::new(url)
        .fit_to_exact_size(egui::vec2(size, size))
        .corner_radius(size / 2.0)
        .paint_at(ui, avatar_rect);
    resp
}

pub(crate) fn badge(ui: &egui::Ui, rect: egui::Rect, count: u32, mention: bool) {
    let painter = ui.painter();
    let center = egui::pos2(rect.right() - 4.0, rect.top() + 6.0);
    let color = if mention {
        egui::Color32::from_rgb(237, 66, 69)
    } else {
        egui::Color32::from_rgb(88, 92, 100)
    };
    painter.circle_filled(center, 9.0, color);
    let text = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(11.0),
        egui::Color32::WHITE,
    );
}

pub(crate) fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = url;
    }
}

pub(crate) fn encode_png(width: usize, height: usize, rgba: &[u8]) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(width as u32, height as u32, rgba.to_vec())?;
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .ok()?;
    Some(out.into_inner())
}

/// Decode a Windows device-independent bitmap (the bytes of CF_DIB / CF_DIBV5)
/// into top-down RGBA8. Supports 24- and 32-bpp uncompressed/bitfield data,
/// which covers screenshots and most copied images.
#[cfg(target_os = "windows")]
pub(crate) fn dib_to_rgba(data: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if data.len() < 40 {
        return None;
    }
    let rd_u32 = |o: usize| -> Option<u32> {
        data.get(o..o + 4)
            .and_then(|s| s.try_into().ok())
            .map(u32::from_le_bytes)
    };
    let rd_i32 = |o: usize| -> Option<i32> {
        data.get(o..o + 4)
            .and_then(|s| s.try_into().ok())
            .map(i32::from_le_bytes)
    };
    let rd_u16 = |o: usize| -> Option<u16> {
        data.get(o..o + 2)
            .and_then(|s| s.try_into().ok())
            .map(u16::from_le_bytes)
    };
    let bi_size = rd_u32(0)? as usize;
    let width = rd_i32(4)?;
    let height = rd_i32(8)?;
    let bpp = rd_u16(14)?;
    let compression = rd_u32(16)?;
    let clr_used = rd_u32(32)? as usize;
    if width <= 0 || height == 0 {
        return None;
    }
    let w = width as usize;
    let h = height.unsigned_abs() as usize;
    let top_down = height < 0;
    let mut offset = bi_size.max(40);
    if bpp <= 8 {
        let n = if clr_used != 0 {
            clr_used
        } else {
            1usize << bpp
        };
        offset += n * 4;
    }
    if compression == 3 && bi_size == 40 {
        offset += 12;
    }
    let mut rgba = vec![0u8; w * h * 4];
    match bpp {
        32 => {
            let row = w * 4;
            if data.len() < offset + row * h {
                return None;
            }
            let has_alpha = (0..h).any(|y| {
                let s = offset + y * row;
                (0..w).any(|x| data[s + x * 4 + 3] != 0)
            });
            for y in 0..h {
                let sy = if top_down { y } else { h - 1 - y };
                let s = offset + sy * row;
                let d = y * row;
                for x in 0..w {
                    let si = s + x * 4;
                    let di = d + x * 4;
                    rgba[di] = data[si + 2];
                    rgba[di + 1] = data[si + 1];
                    rgba[di + 2] = data[si];
                    let a = data[si + 3];
                    rgba[di + 3] = if !has_alpha { 255 } else { a };
                }
            }
        }
        24 => {
            let row = (w * 3).div_ceil(4) * 4;
            if data.len() < offset + row * h {
                return None;
            }
            for y in 0..h {
                let sy = if top_down { y } else { h - 1 - y };
                let s = offset + sy * row;
                let d = y * w * 4;
                for x in 0..w {
                    let si = s + x * 3;
                    let di = d + x * 4;
                    rgba[di] = data[si + 2];
                    rgba[di + 1] = data[si + 1];
                    rgba[di + 2] = data[si];
                    rgba[di + 3] = 255;
                }
            }
        }
        _ => return None,
    }
    Some((w, h, rgba))
}

/// A small title bar with a close button aligned to the right. Returns `true`
/// when the user clicked close.
pub(crate) fn window_header(ui: &mut egui::Ui, title: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(13.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("x").clicked() {
                close = true;
            }
        });
    });
    close
}

/// Discord-style reply indicator: a short accent bar followed by "author: preview".
pub(crate) fn render_reply_line(ui: &mut egui::Ui, author: &str, preview: &str) -> egui::Response {
    let resp = ui
        .horizontal(|ui| {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(3.0, 16.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, 1.5, egui::Color32::from_gray(120));
            ui.add_space(4.0);
            let text = if preview.is_empty() {
                author.to_string()
            } else {
                format!("{author}: {preview}")
            };
            ui.weak(text);
        })
        .response;
    resp.interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Fit a (width, height) image into `max`, preserving aspect ratio.
pub(crate) fn image_fit(w: u32, h: u32, max: egui::Vec2) -> egui::Vec2 {
    let (w, h) = (w as f32, h as f32);
    if w <= 0.0 || h <= 0.0 {
        return egui::vec2(max.x.min(200.0), max.y.min(120.0));
    }
    let s = (max.x / w).min(max.y / h).min(1.0);
    egui::vec2(w * s, h * s)
}

/// Make menu entries flat (no grey button backgrounds), keeping a subtle hover
/// highlight, matching the Discord context-menu look.
pub(crate) fn flatten_menu(ui: &mut egui::Ui) {
    let sel = ui.visuals().selection.bg_fill;
    let w = &mut ui.style_mut().visuals.widgets;
    w.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    w.inactive.bg_stroke = egui::Stroke::NONE;
    w.noninteractive.weak_bg_fill = egui::Color32::TRANSPARENT;
    w.hovered.weak_bg_fill = sel.gamma_multiply(0.6);
    w.hovered.bg_stroke = egui::Stroke::NONE;
    w.active.weak_bg_fill = sel;
}

pub(crate) fn render_plain(ui: &mut egui::Ui, text: &str) {
    if text.is_empty() {
        return;
    }
    for piece in split_emoji(text) {
        match piece {
            EmojiPiece::Text(t) => {
                ui.label(t);
            }
            EmojiPiece::Emoji(e) => match twemoji_url(&e) {
                Some(url) => {
                    ui.add(
                        egui::Image::new(&url)
                            .fit_to_exact_size(egui::vec2(17.0, 17.0)),
                    );
                }
                None => {
                    ui.label(e);
                }
            },
        }
    }
}

pub(crate) fn emoji_button(ui: &mut egui::Ui, ch: &str, size: f32) -> egui::Response {
    match twemoji_url(ch) {
        Some(url) => ui.add(egui::Button::image(
            egui::Image::new(&url).fit_to_exact_size(egui::vec2(size, size)),
        )),
        None => ui.add(egui::Button::new(ch)),
    }
}
