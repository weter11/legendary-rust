use crate::app::LegendaryApp;
use crate::worker::WorkerMsg;
use eframe::egui;
use std::sync::atomic::Ordering;

pub fn show_tasks_view(app: &mut LegendaryApp, ui: &mut egui::Ui) {
    ui.heading("Manager");
    ui.separator();

    if let Some(task) = app.current_task.clone() {
        ui.group(|ui| {
            ui.label(format!("Current Task: {}", task.name));
            ui.add(egui::ProgressBar::new(task.progress).show_percentage());
            ui.horizontal(|ui| {
                if !task.speed.is_empty() {
                    ui.label(format!("Speed: {}", task.speed));
                }
                if !task.eta.is_empty() {
                    ui.label(format!("ETA: {}", task.eta));
                }
            });
            ui.horizontal(|ui| {
                if task.is_paused {
                    if ui.button("Resume").clicked() {
                        if let Some(t) = &mut app.current_task {
                            t.is_paused = false;
                        }
                        app.worker_pause.store(false, Ordering::SeqCst);
                        let _ = app.tx.send(WorkerMsg::ResumeTask);
                    }
                } else {
                    if ui.button("Pause").clicked() {
                        if let Some(t) = &mut app.current_task {
                            t.is_paused = true;
                        }
                        app.worker_pause.store(true, Ordering::SeqCst);
                        let _ = app.tx.send(WorkerMsg::PauseTask);
                    }
                }
                if ui.button("Cancel").clicked() {
                    app.worker_cancel.store(true, Ordering::SeqCst);
                    let _ = app.tx.send(WorkerMsg::CancelTask);
                }
            });
        });
    } else {
        ui.label("No active tasks.");
    }
}
