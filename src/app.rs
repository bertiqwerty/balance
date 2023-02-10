use egui::{Context, Ui};
use std::fmt::Display;
use std::sync::mpsc;
use std::sync::mpsc::Sender;

use crate::blcerr;
use crate::charts::{Chart, Charts};
use crate::compute::{random_walk, RebalanceStats, RebalanceStatsSummary};
use crate::core_types::{to_blc, BlcResult};
use crate::date::{date_after_nmonths, Date};
use crate::io::read_csv_from_str;
use crate::month_slider::{MonthSlider, SliderState};

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
    start_month_slider: MonthSlider,
    n_months: String,
}
impl SimInput {
    fn new() -> Self {
        SimInput {
            vola: Vola::Mi,
            expected_yearly_return: "7.0".to_string(),
            n_months: "360".to_string(),
            start_month_slider: MonthSlider::new(
                Date::new(1950, 1).unwrap(),
                Date::new(2022, 12).unwrap(),
                SliderState::Some(346),
            ),
        }
    }
    fn parse(&self) -> BlcResult<(f64, f64, Date, usize)> {
        Ok((
            self.vola.to_float(),
            self.expected_yearly_return.parse().map_err(to_blc)?,
            self.start_month_slider
                .selected_date()
                .ok_or_else(|| blcerr!("no date selected"))?,
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

// const BASE_URL_WWW: &str = "http://localhost:8000/data";
const BASE_URL_WWW: &str = "https://www.bertiqwerty.com/data";

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
pub struct BalanceApp<'a> {
    rx: mpsc::Sender<ehttp::Result<ehttp::Response>>,
    tx: mpsc::Receiver<ehttp::Result<ehttp::Response>>,
    download: Download<'a>,
    status_msg: Option<String>,
    sim: SimInput,
    charts: Charts,
    payment: PaymentData,
    rebalance_stats: Option<BlcResult<RebalanceStats>>,
    rebalance_stats_summary: Option<BlcResult<RebalanceStatsSummary>>,
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
            rebalance_stats: None,
            rebalance_stats_summary: None,
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
                    let status = format!("{e:?}");
                    self.status_msg = Some(status);
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
    fn recompute_rebalance_stats(&mut self, always: bool) {
        let PaymentData {
            initial_balance: (_, initial_balance),
            monthly_payment: (_, monthly_payment),
            rebalance_interval: (_, rebalance_interval),
        } = self.payment;
        if self.rebalance_stats.is_some() || always {
            if let Some(rebalance_interval) = rebalance_interval {
                let stats = self.charts.compute_rebalancestats(
                    initial_balance,
                    monthly_payment,
                    rebalance_interval,
                );
                if let Ok(stats) = &stats {
                    self.rebalance_stats_summary = Some(stats.mean_across_nmonths());
                }
                self.rebalance_stats = Some(stats);
            } else {
                let err_msg = "no rebalance interval given".to_string();
                self.status_msg = Some(err_msg);
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

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::new([true, true]).show(ui, |ui| {
                ui.heading("Charts");
                egui::CollapsingHeader::new("Simulate").show(ui, |ui| {
                    egui::Grid::new("simulate-inputs")
                        .num_columns(2)
                        .show(ui, |ui| {
                            ui.label("expected yearly return [%]");
                            ui.text_edit_singleline(&mut self.sim.expected_yearly_return);
                            ui.end_row();
                            ui.label("#months");
                            ui.text_edit_singleline(&mut self.sim.n_months);
                            ui.end_row();
                            self.sim.start_month_slider.month_slider(ui, "start date");
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
                            self.rebalance_stats = None;
                            match self.sim.parse() {
                                Ok(data) => {
                                    let (noise, expected_yearly_return, start_date, n_months) =
                                        data;
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
                });
                egui::CollapsingHeader::new("Historical Index Data").show(ui, |ui| {
                    let mut dl_button = |name, filename| {
                        if ui.button(name).clicked() {
                            let url = format!("{BASE_URL_WWW}/{filename}");
                            trigger_dl(&url, self.rx.clone(), ctx.clone());
                            self.download = Download::InProgress(name);
                            self.charts.plot_balance = false;
                            self.rebalance_stats = None;
                        }
                    };
                    dl_button("MSCI ACWI", "msciacwi.csv");
                    dl_button("MSCI World", "msciworld.csv");
                    dl_button("MSCI EM", "msciem.csv");
                    dl_button("MSCI Europe", "mscieurope.csv");
                    dl_button("S&P 500", "sandp500.csv");
                    ui.horizontal(|ui| {
                        ui.label("data from");
                        ui.hyperlink("https://curvo.eu/backtest/")
                    });
                });

                if ui.button("add current chart to balance").clicked() {
                    self.charts.persist_tmp();
                    self.recompute_balance();
                }
                ui.separator();
                ui.heading("Balance");
                egui::Grid::new("inputs-balance-payments-interval")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("initial balance");
                        if ui
                            .text_edit_singleline(&mut self.payment.initial_balance.0)
                            .changed()
                        {
                            self.recompute_balance();
                            self.recompute_rebalance_stats(false);
                        }
                        ui.end_row();
                        ui.label("monthly payment");
                        if ui
                            .text_edit_singleline(&mut self.payment.monthly_payment.0)
                            .changed()
                        {
                            self.recompute_balance();
                            self.recompute_rebalance_stats(false);
                        }
                        ui.end_row();
                        ui.label("rebalance interval (months)");
                        if ui
                            .text_edit_singleline(&mut self.payment.rebalance_interval.0)
                            .changed()
                        {
                            self.recompute_balance();
                            self.recompute_rebalance_stats(false);
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
                                        let total_yield =
                                            balance / (initial_payment + total_monthly);
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
                    });
                egui::CollapsingHeader::new("restrict timeline").show(ui, |ui| {
                    egui::Grid::new("restriction-of-timeline").show(ui, |ui| {
                        if self.charts.start_slider(ui) {
                            self.recompute_balance();
                            self.recompute_rebalance_stats(false);
                        }
                        ui.end_row();
                        if self.charts.end_slider(ui) {
                            self.recompute_balance();
                            self.recompute_rebalance_stats(false);
                        }
                    });
                });
                if !self.charts.persisted.is_empty() {
                    ui.separator();
                    egui::Grid::new("grid-persistend-charts").show(ui, |ui| {
                        if self.charts.fraction_sliders(ui) {
                            self.recompute_balance();
                            self.recompute_rebalance_stats(false);
                        }
                    });
                }
                ui.separator();
                if let Some(status_msg) = &self.status_msg {
                    ui.label(status_msg);
                } else if self.charts.persisted.is_empty() {
                    ui.label("add simulated or historical charts to compute balances");
                } else {
                    ui.label("ready");
                }
                ui.separator();

                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(
                            self.charts.plot_balance && self.rebalance_stats.is_none(),
                            "balance plot",
                        )
                        .clicked()
                    {
                        self.charts.plot_balance = true;
                        self.rebalance_stats = None;
                    }
                    if ui
                        .selectable_label(
                            !self.charts.plot_balance && self.rebalance_stats.is_none(),
                            "charts plot",
                        )
                        .clicked()
                    {
                        self.charts.plot_balance = false;
                        self.rebalance_stats = None;
                    }

                    if ui
                        .selectable_label(self.rebalance_stats.is_some(), "rebalance statistics")
                        .clicked()
                    {
                        self.recompute_rebalance_stats(true);
                    }
                });
                if let (Some(summary), Some(_)) =
                    (&self.rebalance_stats_summary, &self.rebalance_stats)
                {
                    match summary {
                        Ok(summary) => {
                            egui::Grid::new("rebalance-stats").show(ui, |ui| {
                                ui.label("#months");
                                ui.label("w re-balance");
                                ui.label("wo re-balance");
                                ui.label("re-balance is that much better on average");
                                ui.end_row();
                                ui.label(format!(
                                    "{:03} - {:03}",
                                    summary.min_n_months, summary.n_months_33
                                ));
                                ui.label(format!(
                                    "{:0.2}",
                                    summary.mean_across_months_w_reb_min_33
                                ));
                                ui.label(format!(
                                    "{:0.2}",
                                    summary.mean_across_months_wo_reb_min_33
                                ));
                                let factor = summary.mean_across_months_w_reb_min_33
                                    / summary.mean_across_months_wo_reb_min_33;
                                ui.label(format!("{factor:0.3}"));
                                ui.end_row();
                                ui.label(format!(
                                    "{:03} - {:03}",
                                    summary.n_months_33, summary.n_months_67
                                ));
                                ui.label(format!("{:0.2}", summary.mean_across_months_w_reb_33_67));
                                ui.label(format!(
                                    "{:0.2}",
                                    summary.mean_across_months_wo_reb_33_67
                                ));
                                let factor = summary.mean_across_months_w_reb_33_67
                                    / summary.mean_across_months_wo_reb_33_67;
                                ui.label(format!("{factor:0.3}"));
                                ui.end_row();
                                ui.label(format!(
                                    "{:03} - {:03}",
                                    summary.n_months_67, summary.max_n_months
                                ));
                                ui.label(format!(
                                    "{:0.2}",
                                    summary.mean_across_months_w_reb_67_max
                                ));
                                ui.label(format!(
                                    "{:0.2}",
                                    summary.mean_across_months_wo_reb_67_max
                                ));
                                let factor = summary.mean_across_months_w_reb_67_max
                                    / summary.mean_across_months_wo_reb_67_max;
                                ui.label(format!("{factor:0.3}"));
                                ui.end_row();
                                ui.label(format!(
                                    "{:03} - {:03}",
                                    summary.min_n_months, summary.max_n_months
                                ));
                                ui.label(format!("{:0.2}", summary.mean_across_months_w_reb));
                                ui.label(format!("{:0.2}", summary.mean_across_months_wo_reb));
                                let factor = summary.mean_across_months_w_reb
                                    / summary.mean_across_months_wo_reb;
                                ui.label(format!("{factor:0.3}"));
                            });
                            ui.label("We ignore any costs that might be induced by re-balancing.");
                        }
                        Err(e) => {
                            self.status_msg = Some(format!("{e:?}"));
                        }
                    }
                } else if let Err(e) = self.charts.plot(ui, !self.charts.plot_balance) {
                    self.status_msg = Some(format!("{e:?}"));
                }
                egui::warn_if_debug_build(ui);
            });
        });
    }
}
