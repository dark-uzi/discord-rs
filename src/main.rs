mod app;
mod config;
mod gateway;
mod model;
mod rest;
mod state;

use std::sync::{Arc, Mutex};

use eframe::egui;
use tokio::sync::mpsc;

use state::{Command, ConnectionStatus, SharedState};

fn main() -> eframe::Result<()> {
    let shared = Arc::new(Mutex::new(SharedState::default()));
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<Command>();

    // Discord backend: runs on its own tokio runtime in a background thread.
    {
        let shared = shared.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime");

            runtime.block_on(async move {
                let client = rest::http_client();
                while let Some(command) = cmd_rx.recv().await {
                    if let Command::Connect(token) = command {
                        let result =
                            gateway::run_session(shared.clone(), client.clone(), token, &mut cmd_rx)
                                .await;

                        match result {
                            Ok(gateway::SessionEnd::Logout) => {
                                config::clear_token();
                                shared.lock().unwrap().reset();
                            }
                            Ok(gateway::SessionEnd::Shutdown) => {}
                            Err(e) => {
                                let mut s = shared.lock().unwrap();
                                if !matches!(s.status, ConnectionStatus::Error(_)) {
                                    s.status = ConnectionStatus::Error(e.to_string());
                                    s.last_error = Some(e.to_string());
                                }
                            }
                        }
                    }
                }
            });
        });
    }

    // Auto-login with a previously saved token.
    if let Some(token) = config::load_token() {
        let _ = cmd_tx.send(Command::Connect(token));
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([720.0, 480.0])
            .with_title("Discord"),
        ..Default::default()
    };

    eframe::run_native(
        "discord-rs",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);

            // Append Segoe UI Emoji as a fallback so emoji glyphs (incl. ones
            // outside egui's built-in emoji subset) render instead of boxes.
            let mut fonts = egui::FontDefinitions::default();
            if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\seguiemj.ttf") {
                fonts
                    .font_data
                    .insert("segoe_emoji".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
                for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    fonts
                        .families
                        .entry(family)
                        .or_insert_with(Default::default)
                        .push("segoe_emoji".to_owned());
                }
            }
            cc.egui_ctx.set_fonts(fonts);

            Ok(Box::new(app::App::new(shared.clone(), cmd_tx.clone())))
        }),
    )
}
