use crate::app::LegendaryApp;
use crate::worker::WorkerMsg;
use eframe::egui;

pub fn show_auth_view(app: &mut LegendaryApp, ui: &mut egui::Ui) {
    ui.heading("Authentication");
    ui.label(&app.status_message);

    ui.separator();

    ui.collapsing("Login with Authorization Code", |ui| {
        ui.label("How to log in:");
        ui.label(
            "1. Click the button below to open the Epic Games login page in your browser.",
        );
        ui.label(
            "2. After logging in, you will see a JSON response containing 'authorizationCode'.",
        );
        ui.label("3. Copy that code and paste it into the field below.");
        ui.label("4. Click 'Log In' to complete the process.");

        if ui.button("Open Login URL").clicked() {
            let url = crate::api::EgsClient::get_auth_url();
            let _ = open::that(url);
            app.status_message = "Waiting for authorization code...".to_string();
        }

        ui.horizontal(|ui| {
            ui.label("Authorization Code:");
            ui.text_edit_singleline(&mut app.auth_code);
        });

        if ui.button("Log In").clicked() {
            let _ = app.tx.send(WorkerMsg::Login(app.auth_code.clone()));
            app.status_message = "Logging in...".to_string();
        }
    });

    ui.add_space(10.0);

    ui.collapsing("Login with SID", |ui| {
        ui.label("1. Click the button below to open the Epic Games SID page.");
        ui.label("2. You must be already logged in to Epic Games in your browser.");
        ui.label("3. You will see a JSON response containing 'sid'.");
        ui.label("4. Copy that SID and paste it into the field below.");

        if ui.button("Open SID URL").clicked() {
            let _ = open::that("https://www.epicgames.com/id/api/sid");
            app.status_message = "Waiting for SID...".to_string();
        }

        ui.horizontal(|ui| {
            ui.label("SID:");
            ui.text_edit_singleline(&mut app.auth_code);
        });

        if ui.button("Log In with SID").clicked() {
            let _ = app.tx.send(WorkerMsg::LoginSid(app.auth_code.clone()));
            app.status_message = "Logging in with SID...".to_string();
        }
    });
}
