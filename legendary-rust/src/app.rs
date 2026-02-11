use crate::models::{
    AdvancedInfo, Asset, GameInfo, InstalledGame, LibraryItem, OAuthToken, SaveSyncStatus,
    TaskStatus, View,
};
use crate::worker::{spawn_worker, WorkerMsg, WorkerResponse};
use eframe::egui;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};

use crate::config::AppConfig;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub struct LegendaryApp {
    pub(crate) token: Option<OAuthToken>,
    pub(crate) library: Vec<LibraryItem>,
    pub(crate) installed_games: Vec<InstalledGame>,
    pub(crate) assets: Vec<Asset>,
    pub(crate) selected_game: Option<GameInfo>,
    pub(crate) selected_app_name: Option<String>,
    pub(crate) images: HashMap<(String, String), egui_extras::RetainedImage>, // (app_name, type)
    pub(crate) fetching_images: HashSet<(String, String)>,
    pub(crate) config: AppConfig,
    pub(crate) auth_code: String,
    pub(crate) search_query: String,
    pub(crate) status_message: String,
    pub(crate) current_view: View,
    pub(crate) tx: Sender<WorkerMsg>,
    pub(crate) rx: Receiver<WorkerResponse>,
    pub(crate) worker_cancel: Arc<AtomicBool>,
    pub(crate) worker_pause: Arc<AtomicBool>,
    pub(crate) running_apps: HashSet<String>,
    pub(crate) save_sync_status: Option<SaveSyncStatus>,
    pub(crate) unaccepted_eulas: Vec<serde_json::Value>,
    pub(crate) install_info: Option<crate::models::InstallInfo>,
    pub(crate) selected_tags: HashSet<String>,
    pub(crate) current_task: Option<TaskStatus>,
    pub(crate) manifest_files: Vec<String>,
    pub(crate) manifest_search_query: String,
    pub(crate) eos_status: crate::eos::EosOverlayStatus,
    pub(crate) eos_prefix_path: Option<std::path::PathBuf>,
    pub(crate) advanced_info: Option<AdvancedInfo>,
    pub(crate) new_env_key: String,
    pub(crate) new_env_val: String,
}

impl LegendaryApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = channel();
        let (worker_tx, worker_rx) = channel();
        let worker_cancel = Arc::new(AtomicBool::new(false));
        let worker_pause = Arc::new(AtomicBool::new(false));

        spawn_worker(
            tx,
            worker_rx,
            worker_cancel.clone(),
            worker_pause.clone(),
            cc.egui_ctx.clone(),
        );

        let app = Self {
            token: None,
            library: Vec::new(),
            installed_games: crate::auth::load_installed_games(),
            assets: Vec::new(),
            selected_game: None,
            selected_app_name: None,
            images: HashMap::new(),
            fetching_images: HashSet::new(),
            config: AppConfig::load(),
            auth_code: String::new(),
            search_query: String::new(),
            status_message: "Welcome to Legendary Rust".to_string(),
            current_view: View::Auth,
            tx: worker_tx,
            rx,
            worker_cancel,
            worker_pause,
            running_apps: HashSet::new(),
            save_sync_status: None,
            unaccepted_eulas: Vec::new(),
            install_info: None,
            selected_tags: HashSet::new(),
            current_task: None,
            manifest_files: Vec::new(),
            manifest_search_query: String::new(),
            eos_status: crate::eos::EosOverlayStatus::default(),
            eos_prefix_path: None,
            advanced_info: None,
            new_env_key: String::new(),
            new_env_val: String::new(),
        };
        app
    }
}

impl eframe::App for LegendaryApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut style = (*ctx.style()).clone();
        style.wrap = Some(true);
        ctx.set_style(style);
        while let Ok(res) = self.rx.try_recv() {
            match res {
                WorkerResponse::LoggedIn(token) => {
                    self.token = Some(token);
                    self.current_view = View::Library;
                    self.status_message = "Logged in".to_string();
                }
                WorkerResponse::LibraryFetched(items) => {
                    self.library = items;
                }
                WorkerResponse::GameInfoFetched(info) => {
                    if let Some(ref eulas) = info.eula_ids {
                        if !eulas.is_empty() {
                            let _ = self.tx.send(WorkerMsg::CheckEula(eulas.clone()));
                        }
                    }
                    self.selected_game = Some(info);
                    self.current_view = View::GameDetail;
                }
                WorkerResponse::AssetsFetched(assets) => {
                    self.assets = assets;
                }
                WorkerResponse::ImageFetched {
                    app_name,
                    image_type,
                    image,
                } => {
                    let name = format!("{}_{}", app_name, image_type);
                    self.images.insert(
                        (app_name, image_type),
                        egui_extras::RetainedImage::from_color_image(name, image),
                    );
                }
                WorkerResponse::Error(e) => {
                    self.status_message = format!("Error: {}", e);
                    self.current_task = None;
                }
                WorkerResponse::TaskProgress {
                    task_name,
                    progress,
                    is_paused,
                    speed,
                    eta,
                } => {
                    self.current_task = Some(TaskStatus {
                        name: task_name.clone(),
                        progress,
                        is_paused,
                        speed: speed.clone().unwrap_or_default(),
                        eta: eta.clone().unwrap_or_default(),
                    });
                    let mut msg = format!("{}: {:.0}%", task_name, progress * 100.0);
                    if let Some(s) = speed {
                        msg.push_str(&format!(" | {}", s));
                    }
                    if let Some(e) = eta {
                        msg.push_str(&format!(" | ETA: {}", e));
                    }
                    if is_paused {
                        msg.push_str(" (Paused)");
                    }
                    self.status_message = msg;
                }
                WorkerResponse::TaskFinished(msg) => {
                    self.status_message = msg;
                    self.current_task = None;
                }
                WorkerResponse::EulaStatusFetched(eulas) => {
                    self.unaccepted_eulas = eulas;
                }
                WorkerResponse::OriginUriFetched(uri) => {
                    let _ = open::that(uri);
                }
                WorkerResponse::SaveSyncStatusFetched {
                    app_name,
                    files,
                    local_time,
                    remote_time,
                    backup_time,
                    error,
                } => {
                    self.save_sync_status = Some(SaveSyncStatus {
                        app_name,
                        files,
                        local_time,
                        remote_time,
                        backup_time,
                        loading: false,
                        error,
                    });
                }
                WorkerResponse::InstallInfoFetched(info) => {
                    self.selected_tags = info.available_tags.iter().cloned().collect();
                    self.install_info = Some(info);
                    self.current_view = View::InstallDialog;
                }
                WorkerResponse::GamesScanned(games) => {
                    // Check for changes in installation status to refresh images
                    for item in &self.library {
                        let was_installed = self
                            .installed_games
                            .iter()
                            .any(|g| g.app_name == item.app_name);
                        let is_now_installed = games.iter().any(|g| g.app_name == item.app_name);

                        if was_installed != is_now_installed {
                            let local_meta = crate::auth::load_local_metadata(&item.app_name);
                            if let Some(meta) = local_meta {
                                for img_info in &meta.metadata.key_images {
                                    self.images.remove(&(
                                        item.app_name.clone(),
                                        img_info.image_type.clone(),
                                    ));
                                    self.fetching_images.remove(&(
                                        item.app_name.clone(),
                                        img_info.image_type.clone(),
                                    ));
                                }
                            }
                        }
                    }
                    self.installed_games = games;
                }
                WorkerResponse::FilesListed(files) => {
                    self.manifest_files = files;
                }
                WorkerResponse::GameLaunched(app_name) => {
                    self.running_apps.insert(app_name);
                }
                WorkerResponse::GameStopped(app_name) => {
                    self.running_apps.remove(&app_name);
                }
                WorkerResponse::EosStatusFetched(status) => {
                    self.eos_status = status;
                }
                WorkerResponse::AdvancedInfoFetched(info) => {
                    self.advanced_info = Some(info);
                }
            }
        }

        egui::SidePanel::left("side_panel")
            .resizable(true)
            .default_width(150.0)
            .show(ctx, |ui| {
                ui.heading("Legendary Rust");
                ui.add_space(10.0);
                if ui
                    .selectable_label(self.current_view == View::Library, "Library")
                    .clicked()
                {
                    self.current_view = View::Library;
                }
                if ui
                    .selectable_label(self.current_view == View::Settings, "Settings")
                    .clicked()
                {
                    self.current_view = View::Settings;
                }
                if ui
                    .selectable_label(self.current_view == View::Tasks, "Manager")
                    .clicked()
                {
                    self.current_view = View::Tasks;
                }
                if ui
                    .selectable_label(self.current_view == View::EosOverlay, "EOS Overlay")
                    .clicked()
                {
                    self.current_view = View::EosOverlay;
                    let _ = self.tx.send(WorkerMsg::QueryEosStatus { prefix: None });
                }
                if self.token.is_some() {
                    if ui
                        .selectable_label(self.current_view == View::Account, "Account")
                        .clicked()
                    {
                        self.current_view = View::Account;
                    }
                }
                ui.add_space(10.0);
                if self.token.is_none() {
                    if ui.button("Login").clicked() {
                        self.current_view = View::Auth;
                    }
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.current_view {
                View::Auth => crate::ui::auth::show_auth_view(self, ui),
                View::Library => crate::ui::library::show_library_view(self, ui),
                View::GameDetail => crate::ui::game_detail::show_game_detail_view(self, ui),
                View::Settings => crate::ui::settings::show_settings_view(self, ui),
                View::SaveSync => crate::ui::save_sync::show_save_sync_view(self, ui),
                View::InstallDialog => {
                    crate::ui::install_dialog::show_install_dialog_view(self, ui)
                }
                View::Tasks => crate::ui::tasks::show_tasks_view(self, ui),
                View::Account => crate::ui::account::show_account_view(self, ui),
                View::EosOverlay => crate::ui::eos_overlay::show_eos_overlay_view(self, ui),
            }

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(&self.status_message).wrap(true));
                    if let Some(task) = &self.current_task {
                        if self.current_view != View::GameDetail {
                            ui.add(egui::ProgressBar::new(task.progress).show_percentage());
                        }
                    }
                });
                ui.separator();
            });
        });
    }
}
