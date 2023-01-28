use egui::plot::{Line, PlotPoints};
use egui::Context;
use std::mem;
use std::sync::mpsc::Sender;
// use web_sys::{Request, RequestInit, RequestMode, Response};
use std::sync::mpsc;

use crate::compute::random_walk;
use crate::core_types::{to_bres, BalResult};
use crate::io::read_csv_from_str;

#[derive(Debug)]
enum Download {
    None,
    InProgress,
    Done(ehttp::Result<ehttp::Response>),
}

#[derive(PartialEq, Clone)]
enum VolaAmount {
    No,
    Lo,
    Mi,
    Hi,
}

fn trigger_dl(url: &str, rx: Sender<Download>, ctx: Context) {
    let req = ehttp::Request::get(url);
    ehttp::fetch(req, move |response| {
        match rx.send(Download::Done(response)) {
            Ok(_) => {}
            Err(e) => println!("{:#?}", e),
        };
        ctx.request_repaint();
    });
}
struct SimInput {
    vola: VolaAmount,
    expected_yearly_return: String,
    n_months: String,
}
impl SimInput {
    fn new() -> Self {
        SimInput {
            vola: VolaAmount::Mi,
            expected_yearly_return: "".to_string(),
            n_months: "".to_string(),
        }
    }
    fn parse(&self) -> BalResult<(f64, f64, usize)> {
        Ok((
            match self.vola {
                VolaAmount::No => 0.0,
                VolaAmount::Lo => 0.05,
                VolaAmount::Mi => 0.1,
                VolaAmount::Hi => 0.2,
            },
            self.expected_yearly_return.parse().map_err(to_bres)?,
            self.n_months.parse().map_err(to_bres)?,
        ))
    }
}
#[derive(Default, Debug, Clone)]
struct Chart {
    dates: Vec<usize>,
    values: Vec<f64>,
}
impl Chart {
    fn new(dates: Vec<usize>, values: Vec<f64>) -> Self {
        Chart { dates, values }
    }
    fn from_tuple((dates, values): (Vec<usize>, Vec<f64>)) -> Self {
        Self::new(dates, values)
    }
    fn to_line(&self) -> Line {
        Line::new(
            self.dates
                .iter()
                .zip(self.values.iter().enumerate())
                .map(|(_, (i, v))| [i as f64, *v])
                .collect::<PlotPoints>(),
        )
    }
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
pub struct BalanceApp {
    rx: mpsc::Sender<Download>,
    tx: mpsc::Receiver<Download>,
    download: Download,
    status_msg: Option<String>,
    sim_input: SimInput,
    tmp_chart: Chart,
}

impl Default for BalanceApp {
    fn default() -> Self {
        let (rx, tx) = mpsc::channel();
        Self {
            rx,
            tx,
            download: Download::None,
            tmp_chart: Chart::new(vec![], vec![]),
            status_msg: None,
            sim_input: SimInput::new(),
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
            ui.heading("Simulate");
            ui.horizontal(|ui| {
                ui.label("expected yearly return [%]");
                ui.text_edit_singleline(&mut self.sim_input.expected_yearly_return);
            });
            ui.horizontal(|ui| {
                ui.label("vola");
                ui.radio_value(&mut self.sim_input.vola, VolaAmount::No, "no".to_string());
                ui.radio_value(&mut self.sim_input.vola, VolaAmount::Lo, "low".to_string());
                ui.radio_value(&mut self.sim_input.vola, VolaAmount::Mi, "mid".to_string());
                ui.radio_value(&mut self.sim_input.vola, VolaAmount::Hi, "high".to_string());
            });
            ui.horizontal(|ui| {
                ui.label("#months");
                ui.text_edit_singleline(&mut self.sim_input.n_months);
            });
            ui.horizontal(|ui| {
                if ui.button("simulate").clicked() {
                    match self.sim_input.parse() {
                        Ok(data) => {
                            let (noise, expected_yearly_return, n_months) = data;
                            match random_walk(expected_yearly_return, noise, n_months) {
                                Ok(values) => {
                                    self.tmp_chart.values = values;
                                    self.tmp_chart.dates = (0..(n_months + 1)).collect::<Vec<_>>();
                                    self.status_msg = None;
                                }
                                Err(e) => {
                                    self.status_msg = Some(format!("{:?}", e));
                                }
                            };
                        }
                        Err(e) => {
                            self.status_msg = Some(format!("{:?}", e));
                        }
                    };
                }
                if ui.button("add").clicked() {}
            });
            ui.separator();
            ui.heading("Backtest data");
            if ui.button("MSCI EM").clicked() {
                let url = "https://www.bertiqwerty.com/data/msciem.csv";
                trigger_dl(url, self.rx.clone(), ctx.clone());
                self.download = Download::InProgress;
            }
            if ui.button("MSCI World").clicked() {
                let url = "https://www.bertiqwerty.com/data/msciworld.csv";
                trigger_dl(url, self.rx.clone(), ctx.clone());
                self.download = Download::InProgress;
            }

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
            if let Download::InProgress = self.download {
                match self.tx.try_recv() {
                    Ok(d) => {
                        self.download = d;
                        self.status_msg = None;
                    }
                    _ => {
                        self.status_msg = Some("waiting...".to_string());
                    }
                }
            }
            if let Download::Done(d) = &self.download {
                self.tmp_chart = match d {
                    Ok(resp) => Chart::from_tuple(read_csv_from_str(resp.text().unwrap()).unwrap()),
                    Err(e) => {
                        self.status_msg = Some(format!("{:?}", e));
                        mem::take(&mut self.tmp_chart)
                    }
                };
                self.download = Download::None;
            }

            if let Some(status_msg) = &self.status_msg {
                ui.label(status_msg);
            } else {
                ui.label("ready");
            }

            //The central panel the region left after adding TopPanel's and SidePanel's
            egui::plot::Plot::new("month vs price")
                .show(ui, |plot_ui| plot_ui.line(self.tmp_chart.to_line()));

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
