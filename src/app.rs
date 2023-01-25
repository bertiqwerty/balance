use egui::plot::{Line, PlotPoints};
use std::mem;
// use web_sys::{Request, RequestInit, RequestMode, Response};
use std::sync::{mpsc};

use crate::io::read_csv_from_str;

#[derive(Debug)]
enum Download {
    None,
    InProgress,
    Done(ehttp::Result<ehttp::Response>),
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
pub struct BalanceApp {
    rx: mpsc::Sender<Download>,
    tx: mpsc::Receiver<Download>,
    download: Download,
    em_values: Vec<f64>,
    em_dates: Vec<usize>,
    status_msg: Option<String>
}

impl Default for BalanceApp {
    fn default() -> Self {
        let (rx, tx) = mpsc::channel();
        Self {
            rx,
            tx,
            download: Download::None,
            em_values: vec![0.5, 0.75, 1.5],
            em_dates: vec![1, 2, 3],
            status_msg: None,
        }
    }
}

impl BalanceApp {
    /// Called once before the first frame.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        Default::default()
    }
}

impl eframe::App for BalanceApp {
    /// Called by the frame work to save state before shutdown.

    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {

        #[cfg(not(target_arch = "wasm32"))] // no File->Quit on web pages!
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        _frame.close();
                    }
                });
            });
        });

        egui::SidePanel::left("side_panel").show(ctx, |ui| {
            ui.heading("Left ABVC Side Panel");

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label("powered by ");
                    ui.hyperlink_to("egui", "https://github.com/emilk/egui");
                    ui.label(" and ");
                    ui.hyperlink_to(
                        "eframe",
                        "https://github.com/emilk/egui/tree/master/crates/eframe",
                    );
                    ui.label(".");
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            // The central panel the region left after adding TopPanel's and SidePanel's

            if ui.button("load csv").clicked() {
                let url = "https://www.bertiqwerty.com/data/msciem.csv".to_string();

                let ctx = ctx.clone();
                let req = ehttp::Request::get(&url);
                let rx = self.rx.clone();
                ehttp::fetch(req, move |response| {
                    match rx.send(Download::Done(response)) {
                        Ok(_) => {}
                        Err(e) => println!("{:#?}", e),
                    };
                    ctx.request_repaint();
                });
                self.download = Download::InProgress;
            }

            match self.download {
                Download::InProgress => match self.tx.try_recv() {
                    Ok(d) => {
                        self.download = d;
                        self.status_msg = None;
                    }
                    _ => {
                        self.status_msg = Some("waiting...".to_string());
                    }
                },
                _ => {}
            }
            if let Download::Done(d) = &self.download {
                (self.em_dates, self.em_values) = match d {
                    Ok(resp) => read_csv_from_str(resp.text().unwrap()).unwrap(),
                    Err(e) => {
                        self.status_msg = Some(format!("{:?}", e));
                        (
                            mem::take(&mut self.em_dates),
                            mem::take(&mut self.em_values),
                        )
                    }
                };
                self.download = Download::None;
            }

            if let Some(status_msg) = &self.status_msg {
                ui.label(status_msg);
            }

            let line = Line::new(
                self.em_dates
                    .iter()
                    .zip(self.em_values.iter().enumerate())
                    .map(|(_, (i, v))| [i as f64, *v])
                    .collect::<PlotPoints>(),
            );
            //The central panel the region left after adding TopPanel's and SidePanel's
            egui::plot::Plot::new("example_plot")
                .show(ui, |plot_ui| plot_ui.line(line))
                .response;
            ui.label("oink");
            ui.hyperlink("https://github.com/emilk/eframe_template");
            ui.add(egui::github_link_file!(
                "https://github.com/emilk/eframe_template/blob/master/",
                "Source code."
            ));
            egui::warn_if_debug_build(ui);
        });

        if false {
            egui::Window::new("Window").show(ctx, |ui| {
                ui.label("Windows can be moved by dragging them.");
                ui.label("They are automatically sized based on contents.");
                ui.label("You can turn on resizing and scrolling if you like.");
                ui.label("You would normally choose either panels OR windows.");
            });
        }
    }
}
