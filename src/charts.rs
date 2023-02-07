use crate::{
    blcerr,
    compute::{adapt_pricedev_to_initial_balance, compute_balance_over_months, RebalanceData},
    core_types::BlcResult,
    date::{fill_between, n_month_between_dates, Date},
    month_slider::{MonthSlider, SliderState},
};
use egui::{
    plot::{Corner, Legend, Line, PlotPoints},
    Ui,
};
use std::{iter, mem, ops::RangeInclusive};

/// Intersects all timelines of all persisted charts
fn start_end_date<'a>(charts: impl Iterator<Item = &'a Chart> + Clone) -> BlcResult<(Date, Date)> {
    let max_date = &Date::from_str("9999/12").unwrap();
    let min_date = &Date::from_str("0001/01").unwrap();
    let start_date = *charts
        .clone()
        .map(|c| c.dates.first().unwrap_or(min_date))
        .max()
        .ok_or_else(|| blcerr!("no charts added"))?;
    let end_date = *charts
        .map(|c| c.dates.iter().last().unwrap_or(max_date))
        .min()
        .ok_or_else(|| blcerr!("no charts added"))?;
    if end_date <= start_date {
        Err(blcerr!("start date needs to be strictly before enddate"))
    } else {
        Ok((start_date, end_date))
    }
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

fn add_fraction(mut fractions: Vec<f64>) -> Vec<f64> {
    let new_fraction = 1.0 / (1.0 + fractions.len() as f64);
    fractions.push(new_fraction);
    let last_idx = fractions.len() - 1;
    normalize_fractions(fractions, last_idx)
}

fn slice_by_date<'a, T>(
    dates: &[Date],
    start_date: Date,
    end_date: Date,
    to_be_sliced: &'a [T],
) -> BlcResult<&'a [T]> {
    let start_idx = dates
        .iter()
        .position(|d| d >= &start_date)
        .ok_or_else(|| blcerr!("slice by date - could not find start idx of {start_date}"))?;
    let end_idx = dates
        .iter()
        .position(|d| d >= &end_date)
        .ok_or_else(|| blcerr!("slice by date - could not find end idx of {end_date}"))?
        + 1;
    Ok(&to_be_sliced[start_idx..end_idx])
}

fn sync_fraction_strs(fractions: &[f64]) -> Vec<String> {
    fractions
        .iter()
        .map(|fr| format!("{fr:.2}"))
        .collect::<Vec<_>>()
}

#[derive(Default, Debug, Clone)]
pub struct Chart {
    name: String,
    dates: Vec<Date>,
    values: Vec<f64>,
}
impl Chart {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn values(&self) -> &Vec<f64> {
        &self.values
    }

    pub fn new(name: String, dates: Vec<Date>, values: Vec<f64>) -> Self {
        Chart {
            name,
            dates,
            values,
        }
    }

    pub fn from_tuple(name: String, (dates, values): (Vec<Date>, Vec<f64>)) -> Self {
        Self::new(name, dates, values)
    }

    fn to_line(&self, start_date: Date, end_date: Date, start_with_1: bool) -> BlcResult<Line> {
        let sliced_values = self.sliced_values(start_date, end_date)?;
        let line = if start_with_1 {
            adapt_pricedev_to_initial_balance(1.0, sliced_values)
                .enumerate()
                .map(|(i, v)| [i as f64, v])
                .collect::<PlotPoints>()
        } else {
            sliced_values
                .iter()
                .enumerate()
                .map(|(i, v)| [i as f64, *v])
                .collect::<PlotPoints>()
        };
        Ok(Line::new(line).name(self.name.clone()))
    }

    fn sliced_values(&self, start_date: Date, end_date: Date) -> BlcResult<&[f64]> {
        slice_by_date(&self.dates, start_date, end_date, &self.values)
    }

    fn sliced_dates(&self, start_date: Date, end_date: Date) -> BlcResult<&[Date]> {
        slice_by_date(&self.dates, start_date, end_date, &self.dates)
    }
}
#[derive(Default, Clone, Debug)]
pub struct Charts {
    tmp: Chart,
    pub persisted: Vec<Chart>,
    fractions: Vec<f64>,
    pub fraction_strings: Vec<String>,
    total_balance_over_month: Option<Chart>,
    total_payments_over_month: Option<Chart>,
    pub plot_balance: bool,
    pub user_start: MonthSlider,
    pub user_end: MonthSlider,
}
impl Charts {
    pub fn update_start_end_sliders(&mut self) {
        if let Ok((start, end)) = start_end_date(self.persisted.iter().chain(iter::once(&self.tmp)))
        {
            self.user_start = MonthSlider::new(start, end, SliderState::First);
            self.user_end = MonthSlider::new(start, end, SliderState::Last);
        }
    }

    pub fn start_slider(&mut self, ui: &mut Ui) -> bool {
        let contained = self.user_start.month_slider(ui, "restrict start");

        if self.user_start.is_at_end() {
            self.user_start.move_left();
        }
        while self.user_start.is_initialized()
            && self.user_end.selected_date() <= self.user_start.selected_date()
        {
            self.user_end.move_right();
        }
        contained
    }
    pub fn end_slider(&mut self, ui: &mut Ui) -> bool {
        let contained = self.user_end.month_slider(ui, "restrict end");

        if self.user_end.is_at_start() {
            self.user_end.move_right();
        }
        while self.user_end.is_initialized()
            && self.user_end.selected_date() <= self.user_start.selected_date()
        {
            self.user_start.move_left();
        }
        contained
    }

    pub fn n_months_persisted(&self) -> BlcResult<usize> {
        let (start, end) = self.start_end_date(false)?;
        n_month_between_dates(start, end)
    }

    pub fn start_end_date(&self, with_tmp: bool) -> BlcResult<(Date, Date)> {
        let (start, end) = if with_tmp {
            start_end_date(self.persisted.iter().chain(iter::once(&self.tmp)))?
        } else {
            start_end_date(self.persisted.iter())?
        };
        let start = if let Some(user_start) = self.user_start.selected_date() {
            user_start
        } else {
            start
        };
        let end = if let Some(user_end) = self.user_end.selected_date() {
            user_end
        } else {
            end
        };
        if start >= end {
            Err(blcerr!("start needs to be before end"))
        } else {
            Ok((start, end))
        }
    }

    pub fn dates(&self, with_tmp: bool) -> BlcResult<Vec<Date>> {
        let (start, end) = self.start_end_date(with_tmp)?;
        Ok(fill_between(start, end))
    }

    pub fn total_balance_over_month(&self) -> Option<&Chart> {
        self.total_balance_over_month.as_ref()
    }

    pub fn add_tmp(&mut self, mut chart: Chart) {
        chart.name = self.adapt_name(mem::take(&mut chart.name));
        self.tmp = chart;
        self.update_start_end_sliders()
    }

    pub fn move_tmp(&mut self) -> Chart {
        mem::take(&mut self.tmp)
    }

    fn adapt_name(&self, name: String) -> String {
        let exists = self.persisted.iter().any(|ci| ci.name == name);
        if exists {
            format!("{}_{}", name, self.persisted.len())
        } else {
            name
        }
    }

    pub fn persist_tmp(&mut self) {
        if !self.tmp.dates.is_empty() {
            let mut c = mem::take(&mut self.tmp);
            let c = Chart::new(self.adapt_name(mem::take(&mut c.name)), c.dates, c.values);
            self.persisted.push(c);
            self.fractions = add_fraction(mem::take(&mut self.fractions));
            self.fraction_strings = sync_fraction_strs(&self.fractions);
        }
    }

    pub fn remove(&mut self, idx: usize) {
        self.persisted.remove(idx);
        self.fraction_strings.remove(idx);
        let fr_removed = self.fractions.remove(idx);
        let new_fractions = redestribute_fractions(mem::take(&mut self.fractions), fr_removed);
        for (fs, nf) in self.fraction_strings.iter_mut().zip(new_fractions.iter()) {
            *fs = format!("{nf:0.2}");
        }
        self.fractions = new_fractions;
    }

    pub fn compute_balance(
        &mut self,
        initial_balance: f64,
        monthly_payments: f64,
        rebalance_interval: Option<usize>,
    ) -> BlcResult<()> {
        let mut lens = self.persisted.iter().map(|dev| dev.dates.len());
        let first_len = lens.next().ok_or_else(|| blcerr!("no charts added"))?;

        let (start_date, end_date) = self.start_end_date(false)?;
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

    pub fn update_fractions(&mut self, idx: usize) {
        if let Ok(new_fr) = self.fraction_strings[idx].parse::<f64>() {
            self.fractions[idx] = new_fr;
            self.fraction_strings = sync_fraction_strs(&self.fractions);
        }
    }

    pub fn plot(&self, ui: &mut Ui, with_tmp: bool) -> BlcResult<()> {
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
            self.persisted.iter().chain(iter::once(&self.tmp)).collect()
        };

        let dates = match self.dates(with_tmp) {
            Ok(dates) => dates,
            Err(_) => self.tmp.dates.clone(),
        };
        let start_date = dates.first().copied();
        let end_date = dates.last().copied();
        let x_fmt_tbom = move |x: f64, _range: &RangeInclusive<f64>| {
            if x.fract().abs() < 1e-6 {
                let i = x.round() as usize;
                if i < dates.len() {
                    dates[i].to_string()
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
                    if let (Some(start), Some(end)) = (start_date, end_date) {
                        if let Ok(line) = c.to_line(start, end, with_tmp) {
                            plot_ui.line(line);
                        }
                    }
                }
            });
        Ok(())
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
