use crate::compute::{
    random_walk, yearly_return, BestRebalanceTrigger, RebalanceStats, RebalanceStatsSummary,
    RebalanceTrigger,
};
use crate::core_types::{to_blc, BlcResult};
use crate::date::date_after_nmonths;
use crate::io::{
    read_csv_from_str, sessionid_from_link, sessionid_to_link, ResponsePayload, URL_READ_SHARELINK,
    URL_WRITE_SHARELINK,
};
use charts::{Chart, Charts, TmpChart};
use egui::{Context, Response, RichText, Ui};
use month_slider::{MonthSlider, MonthSliderPair, SliderState};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::iter;
mod charts;
mod month_slider;
mod ui_state_types;

#[cfg(not(target_arch = "wasm32"))]
use std::{fs::File, io::Write};

use self::ui_state_types::{
    FinalBalance, ParsedSimInput, PaymentData, RestMethod, RestRequest, RestRequestState, SimInput,
    VolaAmount,
};

#[cfg(target_arch = "wasm32")]
use {
    wasm_bindgen::prelude::*,
    wasm_bindgen::JsValue,
    web_sys::{window, Blob, HtmlElement, Url},
};

// const BASE_URL_WWW: &str = "http://localhost:8000/data";
const BASE_URL_WWW: &str = "https://www.bertiqwerty.com/data";

#[cfg(target_arch = "wasm32")]
fn download_str(s: &str, tmp_filename: &str) -> Result<(), JsValue> {
    let blob = Blob::new_with_str_sequence(&serde_wasm_bindgen::to_value(&[s])?)?;
    let url = Url::create_object_url_with_blob(&blob)?;

    let document = web_sys::window().unwrap().document().unwrap();
    let download_link = document.create_element("a")?.dyn_into::<HtmlElement>()?;
    download_link.set_attribute("href", &url)?;
    download_link.set_attribute("download", tmp_filename)?;
    download_link.click();
    Url::revoke_object_url(&url)?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn get_current_url() -> String {
    window().unwrap().location().href().unwrap()
}

macro_rules! recompute {
    ($self:expr) => {
        $self.best_rebalance_trigger = None;
        $self.recompute_balance();
        $self.recompute_rebalance_stats(false);
    };
}

fn export_csv(charts: &Charts) -> BlcResult<()> {
    let tmp_filename = "charts.csv";

    let s = charts.to_string();

    #[cfg(target_arch = "wasm32")]
    download_str(&s, tmp_filename).map_err(to_blc)?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut tmp_file = File::create(tmp_filename).map_err(to_blc).unwrap();
        write!(tmp_file, "{s}").map_err(to_blc)?;
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

fn remove_indices<T: Clone>(v: &mut Vec<T>, to_be_deleted: &[usize]) {
    let mut target_idx = 0;
    for src_idx in 0..v.len() {
        if !to_be_deleted.contains(&src_idx) {
            if src_idx != target_idx {
                v[target_idx] = v[src_idx].clone();
            }
            target_idx += 1;
        }
    }
    v.truncate(target_idx);
}

fn heading2(ui: &mut Ui, s: &str) -> Response {
    ui.heading(RichText::new(s).strong().size(18.0))
}

fn heading(ui: &mut Ui, s: &str) -> Response {
    ui.heading(RichText::new(s).strong().size(30.0))
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(Deserialize, Serialize, Default)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct BalanceApp<'a> {
    #[serde(skip)]
    download_historic_csv: RestRequest<'a>,
    #[serde(skip)]
    sharelink_request: RestRequest<'a>,
    #[serde(skip)]
    load_request: RestRequest<'a>,
    #[serde(skip)]
    session_id_to_be_loaded: String,
    status_msg: Option<String>,
    sim: SimInput,
    charts: Charts,
    payment: PaymentData,
    rebalance_stats: Option<BlcResult<RebalanceStats>>,
    rebalance_stats_summary: Option<BlcResult<RebalanceStatsSummary>>,
    best_rebalance_trigger: Option<BestRebalanceTrigger>,
    final_balance: Option<FinalBalance>,
}

impl<'a> BalanceApp<'a> {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        let app: Self = if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            // This is also where you can customize the look and feel of egui using
            // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.
            Self::default()
        };
        #[cfg(target_arch = "wasm32")]
        {
            let mut app = app;
            app.get_session_fromurl();
            app
        }
        #[cfg(not(target_arch = "wasm32"))]
        app
    }

    #[cfg(target_arch = "wasm32")]
    fn get_session_fromurl(&mut self) {
        let link_with_sessionid = get_current_url();
        if sessionid_from_link(&link_with_sessionid).is_some() {
            self.trigger_load(&link_with_sessionid, None);
        }
    }

    fn check_csv_download(&mut self) {
        let (status, state) = self.download_historic_csv.check();
        self.download_historic_csv.state = state;
        if let Some(status) = status {
            self.status_msg = Some(status);
        }
        if let RestRequestState::Done((name, d)) = &self.download_historic_csv.state {
            let tmp = match d {
                Ok(resp) => {
                    let (dates, values) = read_csv_from_str(resp.text().unwrap()).unwrap();
                    self.charts.plot_balance = false;
                    Some(TmpChart {
                        chart: Chart::from_tuple(name.to_string(), (dates, values)),
                        initial_balance: self.payment.initial_balance.1,
                    })
                }
                Err(e) => {
                    let status = e.to_string();
                    self.status_msg = Some(status);
                    self.charts.move_tmp()
                }
            };
            self.charts.add_tmp(tmp);
            self.download_historic_csv.state = RestRequestState::None;
            self.status_msg = None;
        }
    }
    fn trigger_sharelink(&mut self, ctx: &Context) {
        let url = URL_WRITE_SHARELINK;
        let name = "sharelink";
        let self_json_string = serde_json::to_string(self).unwrap();
        let json_data = format!("{{\"json_data\": {} }}", self_json_string);
        let method = RestMethod::Post(json_data.into_bytes());
        self.sharelink_request
            .trigger(url, name, method, Some(ctx.clone()));
    }
    fn check_sharelink(&mut self, ui: &mut Ui) {
        let (status, state) = self.sharelink_request.check();
        self.sharelink_request.state = state;
        if let Some(status) = status {
            self.status_msg = Some(status);
        }
        if let RestRequestState::Done((_name, d)) = &self.sharelink_request.state {
            match d {
                Ok(resp) => {
                    if resp.status == 200 {
                        self.status_msg = None;
                        ui.output_mut(|o| {
                            #[derive(Serialize, Deserialize)]
                            struct WriteJsonData {
                                pub session_id: String,
                            }
                            let json_str = resp.text().unwrap();
                            let v: ResponsePayload<WriteJsonData> =
                                serde_json::from_str(json_str).unwrap();
                            let session_id = v.json_data.session_id;
                            o.copied_text = sessionid_to_link(&session_id);
                        });
                        self.sharelink_request.state = RestRequestState::None;
                    } else {
                        let json_str = resp.text().unwrap();
                        let v: Value = serde_json::from_str(json_str).unwrap();
                        let status = format!(
                            "status {}, {}, {}",
                            resp.status,
                            &v["message"].to_string(),
                            resp.status_text.clone()
                        );
                        self.status_msg = Some(status);
                    }
                }
                Err(e) => {
                    let status = e.to_string();
                    self.status_msg = Some(status);
                }
            };
        }
    }

    pub fn trigger_load(&mut self, link_with_sessionid: &str, ctx: Option<&Context>) {
        if let Some(session_id) = sessionid_from_link(link_with_sessionid) {
            let url = format!("{URL_READ_SHARELINK}?session_id={session_id}");
            self.load_request
                .trigger(url.as_str(), "load", RestMethod::Get, ctx.cloned())
        } else {
            self.status_msg = Some(format!(
                "invalid link with session id {link_with_sessionid}"
            ));
        }
    }
    pub fn check_load(&mut self) {
        let (status, state) = self.load_request.check();
        self.load_request.state = state;
        if let Some(status) = status {
            self.status_msg = Some(status);
        }
        if let RestRequestState::Done((_name, d)) = &self.load_request.state {
            match d {
                Ok(resp) => {
                    if resp.status == 200 {
                        let json_str = resp.text().unwrap();
                        let v: ResponsePayload<Self> = serde_json::from_str(json_str).unwrap();
                        let new_balance = v.json_data;
                        *self = new_balance;
                    } else {
                        let json_str = resp.text().unwrap();
                        let v: Value = serde_json::from_str(json_str).unwrap();
                        let status = format!(
                            "status {}, {}, {}",
                            resp.status,
                            &v["message"].to_string(),
                            resp.status_text.clone()
                        );
                        self.status_msg = Some(status);
                    }
                }
                Err(e) => {
                    let status = e.to_string();
                    self.status_msg = Some(status);
                }
            };
        }
    }
    fn recompute_balance(&mut self) {
        if let Err(e) = self.payment.parse() {
            self.status_msg = Some(format!("{e}"));
            self.final_balance = None;
        } else {
            let PaymentData {
                initial_balance: (_, initial_balance),
                monthly_payments,
                rebalance_interval: (_, interval),
                rebalance_deviation: (_, deviation),
            } = &self.payment;
            if let Err(e) = self.charts.compute_balance(
                *initial_balance,
                &monthly_payments.payments,
                RebalanceTrigger {
                    interval: *interval,
                    deviation: *deviation,
                },
            ) {
                self.status_msg = Some(format!("{e}"));
                self.final_balance = None;
            } else {
                self.status_msg = None;
                self.charts.plot_balance = true;
                match (
                    self.charts.total_balance_over_month(),
                    self.charts.n_months_persisted(),
                ) {
                    (Some(tbom), Ok(n_months)) => {
                        let final_balance = FinalBalance::from_chart(
                            tbom,
                            *initial_balance,
                            &monthly_payments.payments,
                            n_months,
                        );
                        match final_balance {
                            Ok(final_balance) => {
                                self.final_balance = Some(final_balance);
                            }
                            Err(e) => {
                                self.status_msg = Some(e.to_string());
                            }
                        }
                    }
                    (_, Err(e)) => {
                        self.status_msg = Some(e.to_string());
                        self.final_balance = None;
                    }
                    (_, _) => {
                        self.final_balance = None;
                    }
                }
            }
        }
    }
    fn recompute_rebalance_stats(&mut self, always: bool) {
        let PaymentData {
            initial_balance: (_, initial_balance),
            monthly_payments,
            rebalance_interval: (_, interval),
            rebalance_deviation: (_, deviation),
        } = &self.payment;
        if self.rebalance_stats.is_some() || always {
            if interval.is_some() || deviation.is_some() {
                let stats = self.charts.compute_rebalancestats(
                    *initial_balance,
                    &monthly_payments.payments,
                    RebalanceTrigger {
                        interval: *interval,
                        deviation: *deviation,
                    },
                );
                if let Ok(stats) = &stats {
                    self.rebalance_stats_summary = Some(stats.mean_across_nmonths());
                }
                self.rebalance_stats = Some(stats);
            } else {
                let err_msg = "neither rebalance interval nor deviation given".to_string();
                self.status_msg = Some(err_msg);
            }
        }
    }
}

impl<'a> eframe::App for BalanceApp<'a> {
    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.check_csv_download();
        self.check_load();

        #[cfg(not(target_arch = "wasm32"))] // no File->Quit on web pages!
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Reset").clicked() {
                        *self = Self::default();
                    }
                    if ui.button("Quit").clicked() {
                        _frame.close();
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.check_sharelink(ui);
            egui::ScrollArea::new([true, true]).show(ui, |ui| {
                heading(ui, "Balance");
                ui.separator();
                let make_text = |txt| egui::RichText::new(txt).code().strong();
                if let Some(status_msg) = &self.status_msg {
                    ui.label(make_text(status_msg.as_str()));
                } else if self.charts.persisted.is_empty() {
                    ui.label(make_text(
                        "Add simulated or historical charts to compute balances",
                    ));
                } else {
                    ui.label(make_text("Balance computation ready"));
                }
                ui.separator();
                heading2(ui, "1. Add Price Development(s)");
                egui::CollapsingHeader::new("Simulate price development").show(ui, |ui| {
                    egui::Grid::new("simulate-inputs")
                        .num_columns(2)
                        .show(ui, |ui| {
                            ui.label("Expected yearly return [%]");
                            ui.text_edit_singleline(&mut self.sim.expected_yearly_return);
                            ui.end_row();
                            ui.label("#Months");
                            ui.text_edit_singleline(&mut self.sim.n_months);
                            ui.end_row();
                            ui.label("Start date");
                            self.sim.start_month_slider.month_slider(ui);
                        });
                    ui.horizontal(|ui| {
                        ui.label("Vola");
                        ui.radio_value(
                            &mut self.sim.vola.amount,
                            VolaAmount::No,
                            format!("{}", VolaAmount::No),
                        );
                        ui.radio_value(
                            &mut self.sim.vola.amount,
                            VolaAmount::Lo,
                            format!("{}", VolaAmount::Lo),
                        );
                        ui.radio_value(
                            &mut self.sim.vola.amount,
                            VolaAmount::Mi,
                            format!("{}", VolaAmount::Mi),
                        );
                        ui.radio_value(
                            &mut self.sim.vola.amount,
                            VolaAmount::Hi,
                            format!("{}", VolaAmount::Hi),
                        );
                    });
                    let mut to_be_deleted = vec![];
                    egui::CollapsingHeader::new("Advanced").show(ui, |ui| {
                        egui::Grid::new("simulate-advanced")
                            .num_columns(2)
                            .show(ui, |ui| {
                                ui.label("Name (auto-generated if empty)");
                                ui.text_edit_singleline(&mut self.sim.name);
                                ui.end_row();
                                ui.label("Return independent of previous returns");
                                ui.checkbox(&mut self.sim.is_eyr_markovian, "");
                                ui.end_row();
                                ui.label("Times of similar volatility");
                                ui.checkbox(&mut self.sim.vola.smoothing, "");
                                ui.end_row();
                                for (i, s) in self.sim.crashes.iter_mut().enumerate() {
                                    ui.label(format!("Crash {}", i + 1));
                                    s.month_slider(ui);
                                    if ui.button("x").clicked() {
                                        to_be_deleted.push(i);
                                    }
                                    ui.end_row();
                                }
                            });
                        if !self.sim.crashes.is_empty() {
                            remove_indices(&mut self.sim.crashes, &to_be_deleted);
                        }
                        if ui.button("Add crash").clicked() {
                            let start_end = self.charts.start_end_date(true);
                            match start_end {
                                Ok(se) => {
                                    let (start, end) = se;
                                    self.sim.crashes.push(MonthSlider::new(
                                        start,
                                        end,
                                        SliderState::First,
                                    ))
                                }
                                Err(_) => {
                                    if let (Some(start), Ok(n_month)) = (
                                        self.sim.start_month_slider.selected_date(),
                                        self.sim.n_months.parse::<usize>(),
                                    ) {
                                        let end = start + n_month;
                                        if let Ok(end) = end {
                                            self.sim.crashes.push(MonthSlider::new(
                                                start,
                                                end,
                                                SliderState::First,
                                            ))
                                        }
                                    } else {
                                        self.status_msg = Some(format!(
                                            "couldn't parse n_month, what integer>0 is {}",
                                            self.sim.n_months
                                        ));
                                    }
                                }
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Run simulation").clicked() {
                            self.rebalance_stats = None;
                            match self.sim.parse() {
                                Ok(parsed) => {
                                    let ParsedSimInput {
                                        vola,
                                        vola_window,
                                        expected_yearly_return,
                                        is_eyr_markovian,
                                        start_month: start_date,
                                        n_months,
                                        mut crashes,
                                    } = parsed;
                                    // remove crashes that are not within relevant timespan
                                    let to_be_del = self
                                        .sim
                                        .crashes
                                        .iter()
                                        .enumerate()
                                        .flat_map(|(idx, c)| c.selected_date().map(|d| (idx, d)))
                                        .filter(|(_, d)| {
                                            d < &start_date
                                                || d > &(start_date + n_months).unwrap_or(*d)
                                        })
                                        .map(|(idx, _)| idx)
                                        .collect::<Vec<_>>();
                                    remove_indices(&mut crashes, &to_be_del);
                                    match random_walk(
                                        expected_yearly_return,
                                        is_eyr_markovian,
                                        vola,
                                        vola_window,
                                        n_months,
                                        &crashes,
                                    ) {
                                        Ok(values) => {
                                            let chart = Chart::new(
                                                if self.sim.name.is_empty() {
                                                    format!(
                                                        "{}_{}_{}_{}",
                                                        self.sim.expected_yearly_return,
                                                        self.sim.n_months,
                                                        self.sim.vola,
                                                        if self.sim.is_eyr_markovian {
                                                            "mrkv"
                                                        } else {
                                                            "non-mrkv"
                                                        }
                                                    )
                                                } else {
                                                    self.sim.name.clone()
                                                },
                                                (0..(n_months + 1))
                                                    .map(|i| date_after_nmonths(start_date, i))
                                                    .collect::<Vec<_>>(),
                                                values,
                                            );
                                            self.charts.add_tmp(Some(TmpChart {
                                                chart,
                                                initial_balance: self.payment.initial_balance.1,
                                            }));
                                            self.status_msg = None;
                                            self.charts.plot_balance = false;
                                        }
                                        Err(e) => {
                                            self.status_msg = Some(format!("{e}"));
                                        }
                                    };
                                }
                                Err(e) => {
                                    self.status_msg = Some(format!("{e}"));
                                }
                            };
                        }
                    });
                });
                egui::CollapsingHeader::new("Use historical data as price development").show(
                    ui,
                    |ui| {
                        ui.horizontal(|ui| {
                            let mut dl_button = |name, filename| {
                                if ui.button(name).clicked() {
                                    let url = format!("{BASE_URL_WWW}/{filename}");
                                    self.download_historic_csv.trigger(
                                        &url,
                                        name,
                                        RestMethod::Get,
                                        Some(ctx.clone()),
                                    );
                                    self.charts.plot_balance = false;
                                    self.rebalance_stats = None;
                                }
                            };
                            dl_button("MSCI ACWI", "msciacwi.csv");
                            dl_button("MSCI World", "msciworld.csv");
                            dl_button("MSCI EM", "msciem.csv");
                            dl_button("MSCI Europe", "mscieurope.csv");
                            dl_button("S&P 500", "sandp500.csv");
                        });
                        ui.horizontal(|ui| {
                            ui.label("data from");
                            ui.hyperlink("https://curvo.eu/backtest/")
                        });
                    },
                );

                if ui
                    .button("Add price development for balance computation")
                    .clicked()
                {
                    self.best_rebalance_trigger = None;
                    self.charts.persist_tmp();
                    self.recompute_balance();
                }
                ui.separator();
                heading2(ui, "2. Set Investments");
                ui.label("Initial balance");
                if ui
                    .text_edit_singleline(&mut self.payment.initial_balance.0)
                    .changed()
                {
                    recompute!(self);
                }
                egui::CollapsingHeader::new("Monthly payments").show(ui, |ui| {
                    egui::Grid::new("monthly-payments-interval")
                        .num_columns(2)
                        .show(ui, |ui| {
                            let mut to_be_deleted = vec![];
                            for i in 0..self.payment.monthly_payments.pay_fields.len() {
                                if i > 0 {
                                    ui.label(format!("Monthly payment {}", i + 1).as_str());
                                } else {
                                    ui.label("Monthly payment");
                                }
                                if ui
                                    .text_edit_singleline(
                                        &mut self.payment.monthly_payments.pay_fields[i],
                                    )
                                    .changed()
                                {
                                    recompute!(self);
                                }
                                if !self.payment.monthly_payments.sliders.is_empty() {
                                    ui.end_row();
                                    ui.label("");
                                    if self.payment.monthly_payments.sliders[i].start_slider(ui) {
                                        recompute!(self);
                                    }
                                    if ui.button("x").clicked() {
                                        to_be_deleted.push(i);
                                    }
                                    ui.end_row();
                                    ui.label("");
                                    if self.payment.monthly_payments.sliders[i].end_slider(ui) {
                                        recompute!(self);
                                    }
                                }
                                ui.end_row();
                            }
                            remove_indices(
                                &mut self.payment.monthly_payments.sliders,
                                &to_be_deleted,
                            );
                            if self.payment.monthly_payments.pay_fields.len() > 1 {
                                remove_indices(
                                    &mut self.payment.monthly_payments.pay_fields,
                                    &to_be_deleted,
                                );
                            }
                            if !to_be_deleted.is_empty() {
                                recompute!(self);
                            }
                            let button_label = if self.payment.monthly_payments.sliders.is_empty() {
                                "Restrict or add"
                            } else {
                                "Add"
                            };
                            if ui.button(button_label).clicked() {
                                let start_end = self.charts.start_end_date(true);
                                match start_end {
                                    Ok(se) => {
                                        if !self.payment.monthly_payments.sliders.is_empty() {
                                            self.payment
                                                .monthly_payments
                                                .pay_fields
                                                .push("0.0".to_string());
                                        }
                                        let (start_date, end_date) = se;
                                        let start_slider = MonthSlider::new(
                                            start_date,
                                            end_date,
                                            SliderState::First,
                                        );
                                        let end_slider = MonthSlider::new(
                                            start_date,
                                            end_date,
                                            SliderState::Last,
                                        );
                                        self.payment
                                            .monthly_payments
                                            .sliders
                                            .push(MonthSliderPair::new(start_slider, end_slider));
                                    }
                                    Err(e) => {
                                        self.status_msg = Some(e.msg.to_string());
                                    }
                                }
                            }
                        });
                });
                egui::CollapsingHeader::new("Rebalancing strategy").show(ui, |ui| {
                    egui::Grid::new("rebalancing-strategy-inputs").show(ui, |ui| {
                        ui.label("Rebalance interval [#months]");
                        if ui
                            .text_edit_singleline(&mut self.payment.rebalance_interval.0)
                            .changed()
                        {
                            self.recompute_balance();
                            self.recompute_rebalance_stats(false);
                        }
                        ui.end_row();
                        ui.label("Rebalance deviation threshold [%]");
                        if ui
                            .text_edit_singleline(&mut self.payment.rebalance_deviation.0)
                            .changed()
                        {
                            self.recompute_balance();
                            self.recompute_rebalance_stats(false);
                        }
                        ui.end_row();
                    });
                });
                egui::CollapsingHeader::new("Restrict timeline").show(ui, |ui| {
                    egui::Grid::new("restriction-of-timeline").show(ui, |ui| {
                        if self.charts.start_slider(ui) {
                            recompute!(self);
                        }
                        ui.end_row();
                        if self.charts.end_slider(ui) {
                            recompute!(self);
                        }
                    });
                });
                if !self.charts.persisted.is_empty() {
                    ui.separator();
                    egui::Grid::new("grid-persistend-charts").show(ui, |ui| {
                        if self.charts.fraction_sliders(ui) {
                            recompute!(self);
                        }
                    });
                }
                ui.separator();
                heading2(ui, "3. Investigate Results of Balance Computation");
                egui::Grid::new("balance-number-results").show(ui, |ui| {
                    if let Some(final_balance) = &self.final_balance {
                        let FinalBalance {
                            final_balance,
                            yearly_return_perc,
                            total_yield,
                        } = final_balance;
                        ui.label("Final balance");
                        ui.label(RichText::new(format!("{final_balance:0.2}")).strong());
                        ui.label("Yearly reaturn [%]");
                        ui.label(RichText::new(format!("{yearly_return_perc:0.2}")).strong());
                        ui.label("Factor");
                        ui.label(RichText::new(format!("{total_yield:0.2}")).strong());
                    } else {
                        ui.label("Final balance");
                        ui.label("-");
                        ui.label("Yearly return [%]");
                        ui.label("-");
                        ui.label("Factor");
                        ui.label("-");
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(
                            self.charts.plot_balance
                                && self.rebalance_stats.is_none()
                                && self.best_rebalance_trigger.is_none(),
                            "Balance plot",
                        )
                        .clicked()
                    {
                        self.charts.plot_balance = true;
                        self.rebalance_stats = None;
                        self.best_rebalance_trigger = None;
                    } else if ui
                        .selectable_label(
                            !self.charts.plot_balance
                                && self.rebalance_stats.is_none()
                                && self.best_rebalance_trigger.is_none(),
                            "Charts plot",
                        )
                        .clicked()
                    {
                        self.charts.plot_balance = false;
                        self.rebalance_stats = None;
                        self.best_rebalance_trigger = None;
                    } else if ui
                        .selectable_label(
                            self.rebalance_stats.is_some() && self.best_rebalance_trigger.is_none(),
                            "Rebalance statistics",
                        )
                        .clicked()
                    {
                        self.best_rebalance_trigger = None;
                        self.recompute_rebalance_stats(true);
                    } else if ui
                        .selectable_label(
                            self.best_rebalance_trigger.is_some(),
                            "Best rebalance strategy",
                        )
                        .clicked()
                    {
                        let PaymentData {
                            initial_balance: (_, initial_balance),
                            monthly_payments,
                            rebalance_interval: (_, _),
                            rebalance_deviation: (_, _),
                        } = &self.payment;
                        self.best_rebalance_trigger = match self
                            .charts
                            .find_bestrebalancetrigger(*initial_balance, &monthly_payments.payments)
                        {
                            Ok(x) => Some(x),
                            Err(e) => {
                                self.status_msg = Some(format!("could not find best trigger; {e}"));
                                None
                            }
                        };
                    }
                });
                if let Some(best_trigger) = &self.best_rebalance_trigger {
                    egui::Grid::new("best-balance").show(ui, |ui| {
                        ui.label("(best) balance");
                        ui.label("(best) yearly return");
                        ui.label("interval [#month]");
                        ui.label("deviation threshold [%]");
                        ui.end_row();
                        let initial_payment = self.payment.initial_balance.1;
                        let monthly_payments = self.payment.monthly_payments.payments.clone();
                        let toshow = iter::once(best_trigger.best)
                            .chain(iter::once(best_trigger.with_best_dev))
                            .chain(iter::once(best_trigger.with_best_interval));
                        for (trigger, balance) in toshow {
                            ui.label(format!("{balance:0.2}"));
                            if let Ok(n_months) = self.charts.n_months_persisted() {
                                let (yearly_return_perc, _) = yearly_return(
                                    initial_payment,
                                    monthly_payments.sum_payments_total(n_months, |x| x),
                                    n_months,
                                    balance,
                                );
                                ui.label(format!("{yearly_return_perc:0.2}"));
                            } else {
                                ui.label("-");
                            }
                            if let Some(interval) = trigger.interval {
                                ui.label(format!("{interval}"));
                            } else {
                                ui.label("None");
                            }
                            if let Some(deviation) = trigger.deviation {
                                let dev_perc = (deviation * 100.0).round() as usize;
                                ui.label(format!("{dev_perc}"));
                            } else {
                                ui.label("None");
                            }
                            ui.end_row();
                        }
                    });
                } else if let (Some(summary), Some(_)) =
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
                            self.status_msg = Some(format!("{e}"));
                        }
                    }
                } else if let Err(e) = self.charts.plot(ui) {
                    self.status_msg = Some(format!("{e}"));
                }
                ui.separator();
                egui::CollapsingHeader::new("Share your balance").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Copy link to clipboard").clicked() {
                            self.trigger_sharelink(ctx);
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            ui.text_edit_singleline(&mut self.session_id_to_be_loaded);
                            if ui.button("Load").clicked() {
                                self.trigger_load(&self.session_id_to_be_loaded.clone(), None);
                            }
                        }
                    });
                    ui.end_row();
                    if ui.button("Download charts as csv").clicked() {
                        #[cfg(target_arch = "wasm32")]
                        log("download csv");
                        export_csv(&self.charts).unwrap();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Reset").clicked() {
                        *self = Self::default();
                    }
                    ui.label("-");
                    ui.label("Code on");
                    ui.hyperlink_to("Github", "https://github.com/bertiqwerty/balance");
                    ui.label("-");
                    ui.hyperlink_to("Impressum", "https://bertiqwerty.com/impressum");
                });
                egui::warn_if_debug_build(ui);
            });
        });
    }
}
