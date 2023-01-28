use egui::plot::{Legend, Line, PlotPoints};
use egui::{Context, Ui};
use std::fmt::Display;
use std::mem;
use std::sync::mpsc::Sender;
// use web_sys::{Request, RequestInit, RequestMode, Response};
use std::sync::mpsc;

use crate::compute::random_walk;
use crate::core_types::{to_bres, BalResult};
use crate::io::read_csv_from_str;

#[derive(Debug)]
enum Download<'a> {
    None,
    InProgress(&'a str),
    Done((&'a str, ehttp::Result<ehttp::Response>)),
}

#[derive(PartialEq, Clone)]
enum Vola {
    No,
    Vo,
    Lo,
    Mi,
    Hi,
}
impl Vola {
    fn to_float(&self) -> f64 {
        match self {
            Vola::No => 0.0,
            Vola::Vo => 0.01,
            Vola::Lo => 0.05,
            Vola::Mi => 0.1,
            Vola::Hi => 0.2,
        }
    }
}
impl Display for Vola {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Vola::No => f.write_str("no"),
            Vola::Vo => f.write_str("vlow"),
            Vola::Lo => f.write_str("low"),
            Vola::Mi => f.write_str("mid"),
            Vola::Hi => f.write_str("high"),
        }
    }
}

fn trigger_dl(url: &str, rx: Sender<ehttp::Result<ehttp::Response>>, ctx: Context) {
    let req = ehttp::Request::get(url);
    ehttp::fetch(req, move |response| {
        match rx.send(response) {
            Ok(_) => {}
            Err(e) => println!("{:#?}", e),
        };
        ctx.request_repaint();
    });
}
struct SimInput {
    vola: Vola,
    expected_yearly_return: String,
    n_months: String,
}
impl SimInput {
    fn new() -> Self {
        SimInput {
            vola: Vola::Mi,
            expected_yearly_return: "".to_string(),
            n_months: "".to_string(),
        }
    }
    fn parse(&self) -> BalResult<(f64, f64, usize)> {
        Ok((
            self.vola.to_float(),
            self.expected_yearly_return.parse().map_err(to_bres)?,
            self.n_months.parse().map_err(to_bres)?,
        ))
    }
}
#[derive(Default, Debug, Clone)]
struct Chart {
    name: String,
    dates: Vec<usize>,
    values: Vec<f64>,
}
impl Chart {
    fn new(name: String, dates: Vec<usize>, values: Vec<f64>) -> Self {
        Chart {
            name,
            dates,
            values,
        }
    }
    fn from_tuple(name: String, (dates, values): (Vec<usize>, Vec<f64>)) -> Self {
        Self::new(name, dates, values)
    }
    fn to_line(&self) -> Line {
        Line::new(
            self.dates
                .iter()
                .zip(self.values.iter().enumerate())
                .map(|(_, (i, v))| [i as f64, *v])
                .collect::<PlotPoints>(),
        )
        .name(self.name.clone())
    }
}

#[derive(Default, Clone, Debug)]
struct Charts {
    tmp: Chart,
    added: Vec<Chart>,
}
impl Charts {
    fn adapt_name(&self, name: String) -> String {
        let exists = self.added.iter().any(|ci| ci.name == name);
        if exists {
            format!("{}_{}", name, self.added.len())
        } else {
            name
        }
    }
    fn persist_tmp(&mut self) {
        let mut c = mem::take(&mut self.tmp);
        let c = Chart::new(self.adapt_name(mem::take(&mut c.name)), c.dates, c.values);
        self.added.push(c);
    }
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
pub struct BalanceApp<'a> {
    rx: mpsc::Sender<ehttp::Result<ehttp::Response>>,
    tx: mpsc::Receiver<ehttp::Result<ehttp::Response>>,
    download: Download<'a>,
    status_msg: Option<String>,
    sim_in: SimInput,
    charts: Charts,
}

impl<'a> Default for BalanceApp<'a> {
    fn default() -> Self {
        let (rx, tx) = mpsc::channel();
        Self {
            rx,
            tx,
            download: Download::None,
            status_msg: None,
            sim_in: SimInput::new(),
            charts: Charts::default(),
        }
    }
}

impl<'a> BalanceApp<'a> {
    /// Called once before the first frame.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        Default::default()
    }
    pub fn plot(&self, ui: &mut Ui) {
        //The central panel the region left after adding TopPanel's and SidePanel's
        egui::plot::Plot::new("month vs price")
            .legend(Legend::default())
            .show(ui, |plot_ui| {
                for c in &self.charts.added {
                    plot_ui.line(c.to_line())
                }
                plot_ui.line(self.charts.tmp.to_line());
            });
    }
}

impl<'a> eframe::App for BalanceApp<'a> {
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
                ui.text_edit_singleline(&mut self.sim_in.expected_yearly_return);
            });
            ui.horizontal(|ui| {
                ui.label("vola");
                ui.radio_value(&mut self.sim_in.vola, Vola::No, format!("{}", Vola::No));
                ui.radio_value(&mut self.sim_in.vola, Vola::Vo, format!("{}", Vola::Vo));
                ui.radio_value(&mut self.sim_in.vola, Vola::Lo, format!("{}", Vola::Lo));
                ui.radio_value(&mut self.sim_in.vola, Vola::Mi, format!("{}", Vola::Mi));
                ui.radio_value(&mut self.sim_in.vola, Vola::Hi, format!("{}", Vola::Hi));
            });
            ui.horizontal(|ui| {
                ui.label("#months");
                ui.text_edit_singleline(&mut self.sim_in.n_months);
            });
            ui.horizontal(|ui| {
                if ui.button("simulate").clicked() {
                    match self.sim_in.parse() {
                        Ok(data) => {
                            let (noise, expected_yearly_return, n_months) = data;
                            match random_walk(expected_yearly_return, noise, n_months) {
                                Ok(values) => {
                                    self.charts.tmp.name = self.charts.adapt_name(format!(
                                        "{}_{}_{}",
                                        self.sim_in.expected_yearly_return,
                                        self.sim_in.n_months,
                                        self.sim_in.vola
                                    ));
                                    self.charts.tmp.values = values;
                                    self.charts.tmp.dates = (0..(n_months + 1)).collect::<Vec<_>>();
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
                if ui.button("add").clicked() {
                    self.charts.persist_tmp();
                }
            });
            ui.separator();
            ui.heading("Backtest data");
            if ui.button("MSCI EM").clicked() {
                let url = "https://www.bertiqwerty.com/data/msciem.csv";
                trigger_dl(url, self.rx.clone(), ctx.clone());
                self.download = Download::InProgress("MSCI EM");
            }
            if ui.button("MSCI World").clicked() {
                let url = "https://www.bertiqwerty.com/data/msciworld.csv";
                trigger_dl(url, self.rx.clone(), ctx.clone());
                self.download = Download::InProgress("MSCI World");
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
            if let Download::InProgress(s) = self.download {
                match self.tx.try_recv() {
                    Ok(d) => {
                        self.download = Download::Done((s, d));
                        self.status_msg = None;
                    }
                    _ => {
                        self.status_msg = Some("waiting...".to_string());
                    }
                }
            }
            if let Download::Done((name, d)) = &self.download {
                self.charts.tmp = match d {
                    Ok(resp) => Chart::from_tuple(
                        name.to_string(),
                        read_csv_from_str(resp.text().unwrap()).unwrap(),
                    ),
                    Err(e) => {
                        self.status_msg = Some(format!("{:?}", e));
                        mem::take(&mut self.charts.tmp)
                    }
                };
                self.download = Download::None;
            }

            if let Some(status_msg) = &self.status_msg {
                ui.label(status_msg);
            } else {
                ui.label("ready");
            }

            self.plot(ui);

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
