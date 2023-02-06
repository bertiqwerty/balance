use egui::{Context, Ui};
use std::fmt::Display;
use std::sync::mpsc;
use std::sync::mpsc::Sender;

use crate::charts::{Chart, Charts};
use crate::compute::random_walk;
use crate::core_types::{to_blc, BlcResult};
use crate::date::{date_after_nmonths, Date};
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
    Lo,
    Mi,
    Hi,
}
impl Vola {
    fn to_float(&self) -> f64 {
        match self {
            Vola::No => 0.0,
            Vola::Lo => 0.005,
            Vola::Mi => 0.01,
            Vola::Hi => 0.02,
        }
    }
}
impl Display for Vola {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Vola::No => f.write_str("no"),
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
            Err(e) => println!("{e:#?}"),
        };
        ctx.request_repaint();
    });
}

struct SimInput {
    vola: Vola,
    expected_yearly_return: String,
    start_month: String,
    n_months: String,
}
impl SimInput {
    fn new() -> Self {
        SimInput {
            vola: Vola::Hi,
            expected_yearly_return: "7.0".to_string(),
            start_month: "1987/12".to_string(),
            n_months: "360".to_string(),
        }
    }
    fn parse(&self) -> BlcResult<(f64, f64, Date, usize)> {
        Ok((
            self.vola.to_float(),
            self.expected_yearly_return.parse().map_err(to_blc)?,
            Date::from_str(&self.start_month)?,
            self.n_months.parse().map_err(to_blc)?,
        ))
    }
}

struct PaymentData {
    initial_balance: (String, f64),
    monthly_payment: (String, f64),
    rebalance_interval: (String, Option<usize>),
}
impl PaymentData {
    fn new() -> Self {
        let initial_balance = 10000.0;
        let monthly_payment = 0.0;
        PaymentData {
            initial_balance: (format!("{initial_balance:0.2}"), initial_balance),
            monthly_payment: (format!("{monthly_payment:0.2}"), monthly_payment),
            rebalance_interval: ("".to_string(), None),
        }
    }
    fn parse(&mut self) -> BlcResult<()> {
        self.initial_balance.1 = self.initial_balance.0.parse().map_err(to_blc)?;
        self.monthly_payment.1 = self.monthly_payment.0.parse().map_err(to_blc)?;
        self.rebalance_interval.1 = self.rebalance_interval.0.parse().ok();
        Ok(())
    }
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
pub struct BalanceApp<'a> {
    rx: mpsc::Sender<ehttp::Result<ehttp::Response>>,
    tx: mpsc::Receiver<ehttp::Result<ehttp::Response>>,
    download: Download<'a>,
    status_msg: Option<String>,
    sim: SimInput,
    charts: Charts,
    payment: PaymentData,
}

impl<'a> Default for BalanceApp<'a> {
    fn default() -> Self {
        let (rx, tx) = mpsc::channel();
        Self {
            rx,
            tx,
            download: Download::None,
            status_msg: None,
            sim: SimInput::new(),
            charts: Charts::default(),
            payment: PaymentData::new(),
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

    fn check_download(&mut self) {
        if let Download::InProgress(s) = self.download {
            match self.tx.try_recv() {
                Ok(d) => {
                    self.download = Download::Done((s, d));
                    self.status_msg = None;
                }
                _ => {
                    self.status_msg = Some("downloading...".to_string());
                }
            }
        } else if let Download::Done((name, d)) = &self.download {
            let tmp = match d {
                Ok(resp) => {
                    let (dates, values) = read_csv_from_str(resp.text().unwrap()).unwrap();
                    self.charts.plot_balance = false;
                    Chart::from_tuple(name.to_string(), (dates, values))
                }
                Err(e) => {
                    self.status_msg = Some(format!("{e:?}"));
                    self.charts.move_tmp()
                }
            };
            self.charts.add_tmp(tmp);
            self.download = Download::None;
        }
    }

    fn recompute_balance(&mut self) {
        if let Err(e) = self.payment.parse() {
            self.status_msg = Some(format!("{e:?}"));
        } else {
            let PaymentData {
                initial_balance: (_, initial_balance),
                monthly_payment: (_, monthly_payment),
                rebalance_interval: (_, rebalance_interval),
            } = self.payment;
            if let Err(e) =
                self.charts
                    .compute_balance(initial_balance, monthly_payment, rebalance_interval)
            {
                self.status_msg = Some(format!("{e:?}"));
            } else {
                self.status_msg = None;
                self.charts.plot_balance = true;
            }
        }
    }
}

impl<'a> eframe::App for BalanceApp<'a> {
    /// Called by the frame work to save state before shutdown.

    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.check_download();

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
            egui::Grid::new("simulate-inputs")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.end_row();
                    ui.label("expected yearly return [%]");
                    ui.text_edit_singleline(&mut self.sim.expected_yearly_return);
                    ui.end_row();
                    ui.label("#months");
                    ui.text_edit_singleline(&mut self.sim.n_months);
                    ui.end_row();
                    ui.label("start date (YYYY/MM)");
                    ui.text_edit_singleline(&mut self.sim.start_month);
                });
            ui.horizontal(|ui| {
                ui.label("vola");
                ui.radio_value(&mut self.sim.vola, Vola::No, format!("{}", Vola::No));
                ui.radio_value(&mut self.sim.vola, Vola::Lo, format!("{}", Vola::Lo));
                ui.radio_value(&mut self.sim.vola, Vola::Mi, format!("{}", Vola::Mi));
                ui.radio_value(&mut self.sim.vola, Vola::Hi, format!("{}", Vola::Hi));
            });
            ui.horizontal(|ui| {
                if ui.button("simulate").clicked() {
                    match self.sim.parse() {
                        Ok(data) => {
                            let (noise, expected_yearly_return, start_date, n_months) = data;
                            match random_walk(expected_yearly_return, noise, n_months) {
                                Ok(values) => {
                                    let tmp = Chart::new(
                                        format!(
                                            "{}_{}_{}",
                                            self.sim.expected_yearly_return,
                                            self.sim.n_months,
                                            self.sim.vola
                                        ),
                                        (0..(n_months + 1))
                                            .map(|i| date_after_nmonths(start_date, i))
                                            .collect::<Vec<_>>(),
                                        values,
                                    );
                                    self.charts.add_tmp(tmp);
                                    self.status_msg = None;
                                    self.charts.plot_balance = false;
                                }
                                Err(e) => {
                                    self.status_msg = Some(format!("{e:?}"));
                                }
                            };
                        }
                        Err(e) => {
                            self.status_msg = Some(format!("{e:?}"));
                        }
                    };
                }
            });
            ui.separator();
            ui.heading("Backtest data");
            if ui.button("MSCI EM").clicked() {
                //let url = "https://www.bertiqwerty.com/data/msciem.csv";
                let url = "http://localhost:8000/data/msciem.csv";
                trigger_dl(url, self.rx.clone(), ctx.clone());
                self.download = Download::InProgress("MSCI EM");
            }
            if ui.button("MSCI World").clicked() {
                //let url = "https://www.bertiqwerty.com/data/msciworld.csv";
                let url = "http://localhost:8000/data/msciworld.csv";
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
            if let Some(status_msg) = &self.status_msg {
                ui.label(status_msg);
            } else {
                ui.label("ready");
            }
            egui::Grid::new("inputs-balance-payments-interval")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("initial balance");
                    if ui
                        .text_edit_singleline(&mut self.payment.initial_balance.0)
                        .changed()
                    {
                        self.recompute_balance();
                    }
                    ui.end_row();
                    ui.label("monthly payment");
                    if ui
                        .text_edit_singleline(&mut self.payment.monthly_payment.0)
                        .changed()
                    {
                        self.recompute_balance();
                    }
                    ui.end_row();
                    ui.label("start date (YYYY/MM)");
                    if ui
                        .text_edit_singleline(&mut self.charts.user_start_str)
                        .changed()
                        && self.charts.update_user_start()
                    {
                        self.recompute_balance();
                    }
                    ui.end_row();
                    ui.label("end date (YYYY/MM)");
                    if ui
                        .text_edit_singleline(&mut self.charts.user_end_str)
                        .changed()
                        && self.charts.update_user_end()
                    {
                        self.recompute_balance();
                    }
                    ui.end_row();
                    ui.label("rebalance interval (months)");
                    if ui
                        .text_edit_singleline(&mut self.payment.rebalance_interval.0)
                        .changed()
                    {
                        self.recompute_balance();
                    }
                    let nobalance = |ui: &mut Ui| {
                        ui.label("final balance");
                        ui.label("-");
                        ui.end_row();
                        ui.label("factor");
                        ui.label("-");
                    };
                    ui.end_row();
                    if let Some(tbom) = self.charts.total_balance_over_month() {
                        if let Some(balance) = tbom.values().iter().last() {
                            ui.label("final balance");
                            ui.label(format!("{balance:0.2}"));
                            ui.end_row();
                            ui.label("factor");
                            let initial_payment = self.payment.initial_balance.1;
                            match self.charts.n_months_persisted() {
                                Ok(n_months) => {
                                    let total_monthly =
                                        self.payment.monthly_payment.1 * (n_months - 1) as f64;
                                    let total_yield = balance / (initial_payment + total_monthly);
                                    ui.label(format!("{total_yield:0.2}"));
                                }
                                Err(e) => {
                                    self.status_msg = Some(format!("{e:?}"));
                                }
                            }
                        } else {
                            nobalance(ui);
                        }
                    } else {
                        nobalance(ui);
                    }
                    ui.end_row();
                    if ui.button("add current chart").clicked() {
                        self.charts.persist_tmp();
                        self.recompute_balance();
                    }
                });
            let chart_inds = 0..(self.charts.persisted.len());
            let mut remove_idx = None;
            egui::Grid::new("grid-persistend-charts").show(ui, |ui| {
                for idx in chart_inds {
                    ui.label(self.charts.persisted[idx].name());
                    if ui
                        .text_edit_singleline(&mut self.charts.fraction_strings[idx])
                        .changed()
                    {
                        self.charts.update_fractions(idx);
                    }
                    if ui.button("x").clicked() {
                        remove_idx = Some(idx);
                    }
                    ui.end_row();
                }
            });

            if let Some(idx) = remove_idx {
                self.charts.remove(idx);
            }

            ui.horizontal(|ui| {
                if ui
                    .selectable_label(self.charts.plot_balance, "balance plot")
                    .clicked()
                {
                    self.charts.plot_balance = true;
                }
                if ui
                    .selectable_label(!self.charts.plot_balance, "charts plot")
                    .clicked()
                {
                    self.charts.plot_balance = false;
                }
            });
            if let Err(e) = self.charts.plot(ui, !self.charts.plot_balance) {
                self.status_msg = Some(format!("{e:?}"));
            }
            egui::warn_if_debug_build(ui);
        });
    }
}
