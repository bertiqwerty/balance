use exmex::parse_val;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::sync::mpsc::{self, Receiver, Sender};

use egui::Context;

use crate::compute::{Expr, MonthlyPayments};
use crate::{
    blcerr,
    compute::yearly_return,
    core_types::{to_blc, BlcResult},
    date::{Date, Interval},
};

use super::ui_mut_itemlist::MutItemList;
use super::{
    charts::Chart,
    month_slider::{MonthSlider, MonthSliderPair, SliderState},
};

#[derive(Debug, Default, Clone)]
pub enum RestRequestState<'a> {
    #[default]
    None,
    InProgress(&'a str),
    Done((&'a str, ehttp::Result<ehttp::Response>)),
}

pub enum RestMethod {
    Get,
    Post(Vec<u8>),
}

pub struct RestRequest<'a> {
    pub state: RestRequestState<'a>,
    tx: Sender<ehttp::Result<ehttp::Response>>,
    rx: Receiver<ehttp::Result<ehttp::Response>>,
}
impl<'a> RestRequest<'a> {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            state: RestRequestState::None,
            tx,
            rx,
        }
    }
    pub fn check(&self) -> (Option<String>, RestRequestState<'a>) {
        if let RestRequestState::InProgress(s) = self.state {
            match self.rx.try_recv() {
                Ok(d) => (None, RestRequestState::Done((s, d))),
                _ => (
                    Some("waiting for REST call...".to_string()),
                    self.state.clone(),
                ),
            }
        } else {
            (None, self.state.clone())
        }
    }
    pub fn trigger(&mut self, url: &str, name: &'a str, method: RestMethod, ctx: Option<Context>) {
        let req = match method {
            RestMethod::Get => ehttp::Request::get(url),
            RestMethod::Post(body) => ehttp::Request::post(url, body),
        };
        let tx = self.tx.clone();
        ehttp::fetch(req, move |response| {
            match tx.send(response) {
                Ok(_) => {}
                Err(e) => println!("{e}"),
            };
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
        self.state = RestRequestState::InProgress(name);
    }
}
impl<'a> Default for RestRequest<'a> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(PartialEq, Clone, Serialize, Deserialize)]
pub struct Vola {
    pub amount: VolaAmount,
    pub smoothing: bool,
    smoothing_window: usize,
}
impl Vola {
    fn amount_as_float(&self) -> f64 {
        self.amount.to_float()
    }
    fn new() -> Self {
        Vola {
            amount: VolaAmount::Mi,
            smoothing: true,
            smoothing_window: 12,
        }
    }
}
impl Display for Vola {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "{}-vola-{}",
            self.amount,
            if self.smoothing { "varies" } else { "global" }
        ))
    }
}
#[derive(PartialEq, Clone, Serialize, Deserialize)]
pub enum VolaAmount {
    No,
    Lo,
    Mi,
    Hi,
}
impl VolaAmount {
    fn to_float(&self) -> f64 {
        match self {
            VolaAmount::No => 0.0,
            VolaAmount::Lo => 0.005,
            VolaAmount::Mi => 0.01,
            VolaAmount::Hi => 0.02,
        }
    }
}
impl Display for VolaAmount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VolaAmount::No => f.write_str("no"),
            VolaAmount::Lo => f.write_str("low"),
            VolaAmount::Mi => f.write_str("mid"),
            VolaAmount::Hi => f.write_str("high"),
        }
    }
}

pub struct ParsedSimInput {
    pub vola: f64,
    pub vola_window: usize,
    pub expected_yearly_return: f64,
    pub is_eyr_markovian: bool,
    pub start_month: Date,
    pub n_months: usize,
    pub crashes: Vec<usize>,
}

#[derive(Serialize, Deserialize)]
pub struct SimInput {
    pub vola: Vola,
    pub expected_yearly_return: String,
    pub is_eyr_markovian: bool,
    pub start_month_slider: MonthSlider,
    pub n_months: String,
    pub name: String,
    pub crashes: MutItemList<MonthSlider>,
}
impl SimInput {
    pub fn parse(&self) -> BlcResult<ParsedSimInput> {
        Ok(ParsedSimInput {
            vola: self.vola.amount_as_float(),
            vola_window: if self.vola.smoothing {
                self.vola.smoothing_window
            } else {
                1
            },
            expected_yearly_return: self.expected_yearly_return.parse().map_err(to_blc)?,
            is_eyr_markovian: self.is_eyr_markovian,
            start_month: self
                .start_month_slider
                .selected_date()
                .ok_or_else(|| blcerr!("no date selected"))?,
            n_months: self.n_months.parse().map_err(to_blc)?,
            crashes: self
                .crashes
                .iter()
                .flat_map(|slider| slider.slider_idx())
                .collect(),
        })
    }
}
impl Default for SimInput {
    fn default() -> Self {
        SimInput {
            vola: Vola::new(),
            expected_yearly_return: "6.0".to_string(),
            is_eyr_markovian: false,
            n_months: "360".to_string(),
            start_month_slider: MonthSlider::new(
                Date::new(1970, 1).unwrap(),
                Date::new(2050, 12).unwrap(),
                SliderState::Some(480),
            ),
            name: "".to_string(),
            crashes: MutItemList::default(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyPaymentState {
    pub payments: MonthlyPayments,
    pub pay_fields: Vec<String>,
    pub sliders: Vec<MonthSliderPair>,
}
impl MonthlyPaymentState {
    fn new() -> Self {
        let payment = 0.0;
        let payment_str = format!("{payment:0.2}");
        Self {
            payments: MonthlyPayments::from_single_payment(parse_val(&payment_str).unwrap()),
            pay_fields: vec![payment_str],
            sliders: vec![],
        }
    }
    fn parse(&mut self) -> BlcResult<()> {
        let payments = self
            .pay_fields
            .iter()
            .map(|ps| parse_val::<i32, f64>(ps).map_err(to_blc))
            .collect::<BlcResult<Vec<Expr>>>()?;
        let ok_or_date =
            |d: Option<Date>| d.ok_or_else(|| blcerr!("no date selected for monthly payment"));
        let intervals = self
            .sliders
            .iter()
            .map(|slider_pair| {
                Interval::new(
                    ok_or_date(slider_pair.selected_start_date())?,
                    ok_or_date(slider_pair.selected_end_date())?,
                )
            })
            .collect::<BlcResult<Vec<Interval>>>()?;
        self.payments = if intervals.is_empty() && payments.len() == 1 {
            MonthlyPayments::from_single_payment(payments[0].clone())
        } else {
            MonthlyPayments::from_intervals(payments, intervals)?
        };
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentData {
    pub initial_balance: (String, f64),
    pub monthly_payments: MonthlyPaymentState,
    pub rebalance_interval: (String, Option<usize>),
    pub rebalance_deviation: (String, Option<f64>),
}
impl PaymentData {
    pub fn parse(&mut self) -> BlcResult<()> {
        self.initial_balance.1 = self.initial_balance.0.replace(" ", "").parse().map_err(to_blc)?;
        self.monthly_payments.parse()?;
        self.rebalance_interval.1 = self.rebalance_interval.0.parse().ok();
        self.rebalance_deviation.1 = self
            .rebalance_deviation
            .0
            .parse()
            .ok()
            .map(|d: f64| d / 100.0);
        Ok(())
    }
}
impl Default for PaymentData {
    fn default() -> Self {
        let initial_balance = 10000.0;
        PaymentData {
            initial_balance: (format!("{initial_balance:0.2}"), initial_balance),
            monthly_payments: MonthlyPaymentState::new(),
            rebalance_interval: ("".to_string(), None),
            rebalance_deviation: ("".to_string(), None),
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct FinalBalance {
    pub final_balance: f64,
    pub yearly_return_perc: Option<f64>,  // Option since this might be NAN and json makes NANs to nulls
    pub total_payments: f64,
}
impl FinalBalance {
    pub fn from_chart(price_dev: &Chart, payments: &Chart, n_months: usize) -> BlcResult<Self> {
        if let (Some(final_balance), Some(total_payments)) = (
            price_dev.values().iter().last().copied(),
            payments.values().iter().last().copied(),
        ) {
            let (yearly_return_perc, _) = yearly_return(total_payments, n_months, final_balance);
            Ok(FinalBalance {
                final_balance,
                yearly_return_perc: Some(yearly_return_perc),
                total_payments,
            })
        } else {
            Err(blcerr!("cannot compute final balance from empty chart"))
        }
    }
}
