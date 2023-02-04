use egui::plot::{Corner, Legend, Line, PlotPoints};
use egui::{Context, Ui};
use std::fmt::Display;
use std::mem;
use std::ops::RangeInclusive;
use std::sync::mpsc;
use std::sync::mpsc::Sender;

use crate::blcerr;
use crate::compute::{
    compute_balance_over_months, random_walk, RebalanceData, _adapt_pricedev_to_initial_balance,
};
use crate::core_types::{to_blc, BlcResult};
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
    VryLo,
    Lo,
    Mi,
    Hi,
}
impl Vola {
    fn to_float(&self) -> f64 {
        match self {
            Vola::No => 0.0,
            Vola::VryLo => 0.01,
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
            Vola::VryLo => f.write_str("very low"),
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

fn sorted_indices(v: &[f64]) -> Vec<usize> {
    let mut inds = (0..v.len()).collect::<Vec<_>>();
    inds.sort_by(|i, j| v[*i].partial_cmp(&v[*j]).unwrap());
    inds
}

fn normalize_fractions(mut fractions: Vec<f64>, pivot_idx: usize) -> Vec<f64> {
    let mut rest = 0.0;
    let new_fraction_reduction = fractions[pivot_idx] / (fractions.len() - 1) as f64;
    for idx in sorted_indices(&fractions)
        .iter()
        .filter(|idx| **idx != pivot_idx)
    {
        fractions[*idx] -= new_fraction_reduction + rest;
        fractions[*idx] = if fractions[*idx] < 0.0 {
            rest += fractions[*idx].abs();
            0.0
        } else {
            fractions[*idx]
        };
    }
    fractions
}

fn redestribute_fractions(mut fractions: Vec<f64>, to_redestribute: f64) -> Vec<f64> {
    let mut rest = 0.0;
    let new_fraction_increase = to_redestribute / fractions.len() as f64;
    for idx in sorted_indices(&fractions).iter().rev() {
        fractions[*idx] += new_fraction_increase + rest;
        fractions[*idx] = if fractions[*idx] > 1.0 {
            rest += fractions[*idx] - 1.0;
            1.0
        } else {
            fractions[*idx]
        };
    }
    fractions
}

fn add_fraction(mut fractions: Vec<f64>) -> Vec<f64> {
    let new_fraction = 1.0 / (1.0 + fractions.len() as f64);
    fractions.push(new_fraction);
    let last_idx = fractions.len() - 1;
    normalize_fractions(fractions, last_idx)
}

struct SimInput {
    start_value: String,
    vola: Vola,
    expected_yearly_return: String,
    start_month: String,
    n_months: String,
}
impl SimInput {
    fn new() -> Self {
        SimInput {
            start_value: "1.0".to_string(),
            vola: Vola::Mi,
            expected_yearly_return: "7.0".to_string(),
            start_month: "1987/12".to_string(),
            n_months: "180".to_string(),
        }
    }
    fn parse(&self) -> BlcResult<(f64, f64, f64, usize, usize)> {
        let start_year = &self.start_month[..4].parse::<usize>().map_err(to_blc)?;
        let start_month = self.start_month[5..].parse::<usize>().map_err(to_blc)?;
        if start_month == 0 || start_month > 12 {
            Err(blcerr!("there are only 12 months"))
        } else {
            let start_month = start_year * 100 + start_month;
            Ok((
                self.start_value.parse().map_err(to_blc)?,
                self.vola.to_float(),
                self.expected_yearly_return.parse().map_err(to_blc)?,
                start_month,
                self.n_months.parse().map_err(to_blc)?,
            ))
        }
    }
}
fn slice_by_date<'a, T>(
    dates: &[usize],
    start_date: usize,
    end_date: usize,
    to_be_sliced: &'a [T],
) -> BlcResult<&'a [T]> {
    let start_idx = dates
        .iter()
        .position(|d| d >= &start_date)
        .ok_or_else(|| blcerr!("could not find start idx of {start_date}"))?;
    let end_idx = dates
        .iter()
        .position(|d| d >= &end_date)
        .ok_or_else(|| blcerr!("could not find end idx of {end_date}"))?
        + 1;
    Ok(&to_be_sliced[start_idx..end_idx])
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
            self.values
                .iter()
                .enumerate()
                .map(|(i, v)| [i as f64, *v])
                .collect::<PlotPoints>(),
        )
        .name(self.name.clone())
    }

    fn sliced_values(&self, start_date: usize, end_date: usize) -> BlcResult<&[f64]> {
        slice_by_date(&self.dates, start_date, end_date, &self.values)
    }
    fn sliced_dates(&self, start_date: usize, end_date: usize) -> BlcResult<&[usize]> {
        slice_by_date(&self.dates, start_date, end_date, &self.dates)
    }
}

#[derive(Default, Clone, Debug)]
struct Charts {
    tmp: Chart,
    persisted: Vec<Chart>,
    fractions: Vec<f64>,
    fraction_strings: Vec<String>,
    total_balance_over_month: Option<Chart>,
    total_payments_over_month: Option<Chart>,
    plot_balance: bool,
}
impl Charts {
    fn adapt_name(&self, name: String) -> String {
        let exists = self.persisted.iter().any(|ci| ci.name == name);
        if exists {
            format!("{}_{}", name, self.persisted.len())
        } else {
            name
        }
    }
    fn persist_tmp(&mut self) {
        if !self.tmp.dates.is_empty() {
            let mut c = mem::take(&mut self.tmp);
            let c = Chart::new(self.adapt_name(mem::take(&mut c.name)), c.dates, c.values);
            self.persisted.push(c);
            let new_fractions = add_fraction(mem::take(&mut self.fractions));
            self.set_fractions(new_fractions);
        }
    }

    fn remove(&mut self, idx: usize) {
        self.persisted.remove(idx);
        self.fraction_strings.remove(idx);
        let fr_removed = self.fractions.remove(idx);
        let new_fractions = redestribute_fractions(mem::take(&mut self.fractions), fr_removed);
        for (fs, nf) in self.fraction_strings.iter_mut().zip(new_fractions.iter()) {
            *fs = format!("{nf:0.2}");
        }
        self.fractions = new_fractions;
    }

    /// Intersects all timelines of all persisted charts
    fn start_end_date(&self) -> BlcResult<(usize, usize)> {
        let start_date = *self
            .persisted
            .iter()
            .map(|c| c.dates.first().unwrap_or(&usize::MAX))
            .max()
            .ok_or_else(|| blcerr!("no charts added"))?;
        let end_date = *self
            .persisted
            .iter()
            .map(|c| c.dates.iter().last().unwrap_or(&0))
            .min()
            .ok_or_else(|| blcerr!("no charts added"))?;
        if end_date <= start_date {
            Err(blcerr!("start date needs to be strictly before enddate"))
        } else {
            Ok((start_date, end_date))
        }
    }

    fn compute_balance(
        &mut self,
        initial_balance: f64,
        monthly_payments: f64,
        rebalance_interval: Option<usize>,
    ) -> BlcResult<()> {
        let mut lens = self.persisted.iter().map(|dev| dev.dates.len());
        let first_len = lens.next().ok_or_else(|| blcerr!("no charts added"))?;

        let (start_date, end_date) = self.start_end_date()?;
        let price_devs = self
            .persisted
            .iter()
            .map(|c| c.sliced_values(start_date, end_date))
            .collect::<BlcResult<Vec<_>>>()?;

        let initial_balances = self
            .fractions
            .iter()
            .map(|fr| fr * initial_balance)
            .collect::<Vec<_>>();
        let monthly_payments = self
            .fractions
            .iter()
            .map(|fr| vec![monthly_payments * *fr; first_len - 1])
            .collect::<Vec<_>>();
        let monthly_payments_refs = monthly_payments
            .iter()
            .map(|mp| &mp[..])
            .collect::<Vec<_>>();
        let balance_over_month = compute_balance_over_months(
            &price_devs,
            &initial_balances,
            Some(&monthly_payments_refs),
            rebalance_interval.map(|ri| RebalanceData {
                interval: ri,
                fractions: &self.fractions,
            }),
        )?;
        let (balances, payments): (Vec<f64>, Vec<f64>) = balance_over_month.unzip();
        let dates = self.persisted[0]
            .sliced_dates(start_date, end_date)?
            .to_vec();
        let b_chart = Chart::new("total balances".to_string(), dates.clone(), balances);
        let p_chart = Chart::new("total payments".to_string(), dates, payments);
        self.total_balance_over_month = Some(b_chart);
        self.total_payments_over_month = Some(p_chart);
        Ok(())
    }

    fn set_fractions(&mut self, fractions: Vec<f64>) {
        self.fraction_strings = fractions
            .iter()
            .map(|fr| format!("{fr:.2}"))
            .collect::<Vec<_>>();
        self.fractions = fractions;
    }

    fn plot(&self, ui: &mut Ui) -> BlcResult<()> {
        let charts_to_plot = if self.plot_balance {
            if let (Some(balances), Some(payments)) = (
                &self.total_balance_over_month,
                &self.total_payments_over_month,
            ) {
                vec![balances, payments]
            } else {
                vec![]
            }
        } else {
            let mut pref = self.persisted.iter().collect::<Vec<_>>();
            pref.push(&self.tmp);
            pref
        };

        let dates_clone = if let Some(tbom) = &self.total_balance_over_month {
            tbom.dates.clone()
        } else {
            self.tmp.dates.clone()
        };
        let x_fmt_tbom = move |x: f64, _range: &RangeInclusive<f64>| {
            if x.fract().abs() < 1e-6 {
                let i = x.round() as usize;
                if i < dates_clone.len() {
                    let d = dates_clone[i];
                    let year = d / 100;
                    let month = d % 100;
                    format!("{year:04}/{month:02}")
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        };
        egui::plot::Plot::new("month vs price")
            .legend(Legend::default().position(Corner::LeftTop))
            .show_x(false)
            .x_axis_formatter(x_fmt_tbom)
            .show(ui, |plot_ui| {
                for c in charts_to_plot {
                    plot_ui.line(c.to_line())
                }
            });
        Ok(())
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
    fn plot(&self, ui: &mut Ui) -> BlcResult<()> {
        //The central panel the region left after adding TopPanel's and SidePanel's
        self.charts.plot(ui)?;
        Ok(())
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
            self.charts.tmp = match d {
                Ok(resp) => {
                    let (dates, values) = read_csv_from_str(resp.text().unwrap()).unwrap();
                    Chart::from_tuple(name.to_string(), (dates, values))
                }
                Err(e) => {
                    self.status_msg = Some(format!("{e:?}"));
                    mem::take(&mut self.charts.tmp)
                }
            };
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
                    ui.label("start value");
                    ui.text_edit_singleline(&mut self.sim.start_value);
                    ui.end_row();
                    ui.label("expected yearly return [%]");
                    ui.text_edit_singleline(&mut self.sim.expected_yearly_return);
                    ui.end_row();
                    ui.label("#months");
                    ui.text_edit_singleline(&mut self.sim.n_months);
                    ui.end_row();
                    ui.label("start month (YYYY/MM)");
                    ui.text_edit_singleline(&mut self.sim.start_month);
                });
            ui.horizontal(|ui| {
                ui.label("vola");
                ui.radio_value(&mut self.sim.vola, Vola::No, format!("{}", Vola::No));
                ui.radio_value(&mut self.sim.vola, Vola::VryLo, format!("{}", Vola::VryLo));
                ui.radio_value(&mut self.sim.vola, Vola::Lo, format!("{}", Vola::Lo));
                ui.radio_value(&mut self.sim.vola, Vola::Mi, format!("{}", Vola::Mi));
                ui.radio_value(&mut self.sim.vola, Vola::Hi, format!("{}", Vola::Hi));
            });
            ui.horizontal(|ui| {
                if ui.button("simulate").clicked() {
                    match self.sim.parse() {
                        Ok(data) => {
                            let (start_value, noise, expected_yearly_return, start_month, n_months) = data;
                            match random_walk(expected_yearly_return, noise, n_months) {
                                Ok(values) => {
                                    let values = _adapt_pricedev_to_initial_balance(start_value, &values).collect::<Vec<_>>();
                                    self.charts.tmp.name = self.charts.adapt_name(format!(
                                        "{}_{}_{}",
                                        self.sim.expected_yearly_return,
                                        self.sim.n_months,
                                        self.sim.vola
                                    ));
                                    self.charts.tmp.values = values;
                                    self.charts.tmp.dates = (0..(n_months + 1)).map(|i| {
                                        let start_year = start_month / 100;
                                        let start_month = start_month % 100;

                                        let n_start_months = start_year * 12 + start_month;
                                        let n_years = (n_start_months + i) / 12;
                                        let n_months = (n_start_months + i) % 12;
                                        n_years * 100 + n_months
                                    } ).collect::<Vec<_>>();
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
                    if let Some(tbom) = &self.charts.total_balance_over_month {
                        if let Some(balance) = tbom.values.iter().last() {
                            ui.label("final balance");
                            ui.label(format!("{balance:0.2}"));
                            ui.end_row();
                            ui.label("factor");
                            let total_yield = balance
                                / (self.payment.initial_balance.1
                                    + self.payment.monthly_payment.1
                                        * (tbom.dates.len() - 1) as f64);
                            ui.label(format!("{total_yield:0.2}"));
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
                    ui.label(&self.charts.persisted[idx].name);
                    if ui
                        .text_edit_singleline(&mut self.charts.fraction_strings[idx])
                        .changed()
                    {
                        if let Ok(new_fr) = self.charts.fraction_strings[idx].parse::<f64>() {
                            let mut fractions = mem::take(&mut self.charts.fractions);
                            fractions[idx] = new_fr;
                            self.charts.set_fractions(fractions);
                        }
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
            if let Err(e) = self.plot(ui) {
                self.status_msg = Some(format!("{e:?}"));
            }
            egui::warn_if_debug_build(ui);
        });
    }
}

#[test]
fn test_add_fraction() {
    fn assert_v(v: &[f64], v_ref: &[f64]) {
        for (vi, vrefi) in v.iter().zip(v_ref.iter()) {
            assert!((vi - vrefi).abs() < 1e-12);
        }
    }
    let fracs = vec![];
    assert_v(&add_fraction(fracs), &vec![1.0]);
    let fracs = vec![1.0];
    assert_v(&add_fraction(fracs), &vec![0.5, 0.5]);
    let fracs = vec![0.5, 0.5];
    assert_v(&add_fraction(fracs), &vec![1.0 / 3.0; 3]);
}

#[test]
fn test_sorted_inds() {
    let v = vec![0.4, 123.3, 0.2, -1.0, 0.0];
    let inds = sorted_indices(&v);
    assert_eq!(vec![3, 4, 2, 0, 1], inds);
}

#[test]
fn test_redistribute() {
    let frs = vec![0.1, 0.6, 0.1];
    let x = redestribute_fractions(frs, 0.2);
    assert!((x.iter().sum::<f64>() - 1.0).abs() < 1e-12);
}
