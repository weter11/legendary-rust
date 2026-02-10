use crate::app::LegendaryApp;
use crate::models::{View};
use crate::worker::WorkerMsg;
use eframe::egui;

pub fn show_account_view(app: &mut LegendaryApp, ui: &mut egui::Ui) {
    ui.heading("User Account Overview");
    ui.separator();

    if let Some(token) = &app.token {
        egui::Grid::new("account_grid")
            .num_columns(2)
            .spacing([40.0, 10.0])
            .show(ui, |ui| {
                ui.label("Display Name:");
                ui.label(token.display_name.as_deref().unwrap_or("Unknown"));
                ui.end_row();

                ui.label("Account ID:");
                ui.label(&token.account_id);
                ui.end_row();

                ui.label("Client ID:");
                ui.label(&token.client_id);
                ui.end_row();

                ui.label("Token Type:");
                ui.label(&token.token_type);
                ui.end_row();

                ui.label("Expires At:");
                ui.label(&token.expires_at);
                ui.end_row();
            });

        ui.add_space(20.0);
        if ui.button("Logout").clicked() {
            let _ = app.tx.send(WorkerMsg::Logout);
            app.token = None;
            app.library.clear();
            app.images.clear();
            app.status_message = "Logged out".to_string();
            app.current_view = View::Auth;
        }
    } else {
        ui.label("Not logged in.");
        if ui.button("Go to Login").clicked() {
            app.current_view = View::Auth;
        }
    }
}
