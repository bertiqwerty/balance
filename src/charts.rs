use crate::{
    blcerr,
    compute::{
        adapt_pricedev_to_initial_balance, best_rebalance_trigger, compute_balance_over_months,
        find_shortestlen, rebalance_stats, BestRebalanceTrigger, RebalanceData, RebalanceStats,
        RebalanceTrigger,
    },
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

fn clamp_01(x: f64) -> (f64, f64) {
    if x > 1.0 {
        (1.0, x - 1.0)
    } else if x < 0.0 {
        (0.0, x)
    } else {
        (x, 0.0)
    }
}

fn normalize_fractions(mut fractions: Vec<f64>, pivot_idx: usize, fixed: &[bool]) -> Vec<f64> {
    let n_fixed = fixed
        .iter()
        .enumerate()
        .filter(|(i, b)| *i != pivot_idx && **b)
        .count();
    let fixed_sum: f64 = fractions
        .iter()
        .zip(fixed.iter())
        .enumerate()
        .filter(|(i, (_, b))| **b && *i != pivot_idx)
        .map(|(_, (fr, _))| fr)
        .sum();
    if fractions.len() == 1 {
        fractions[pivot_idx] = 1.0;
        fractions
    } else if fractions.is_empty() {
        fractions
    } else if fractions.len() - n_fixed == 1 {
        fractions[pivot_idx] = 1.0 - fixed_sum;
        fractions
    } else {
        let upper = 1.0 - fixed_sum;

        fractions[pivot_idx] = if fractions[pivot_idx] > upper {
            upper
        } else if fractions[pivot_idx] < 0.0 {
            0.0
        } else {
            fractions[pivot_idx]
        };

        fn is_mutable(i: usize, pivot_idx: usize, fixed: &[bool]) -> bool {
            i != pivot_idx && !fixed[i]
        }

        let mutable_sum: f64 = fractions
            .iter()
            .enumerate()
            .filter(|(i, _)| is_mutable(*i, pivot_idx, fixed))
            .map(|(_, x)| x)
            .sum();
        let to_be_distributed_per_fr = (1.0 - fractions[pivot_idx] - mutable_sum - fixed_sum)
            / (fractions.len() - 1 - n_fixed) as f64;

        fn update<'a, I: Iterator<Item = &'a usize>>(
            it: I,
            pivot_idx: usize,
            fractions: &mut [f64],
            to_be_distributed_per_fr: f64,
            fixed: &[bool],
        ) {
            let mut rest = 0.0;
            for i in it.filter(|i| is_mutable(**i, pivot_idx, fixed)) {
                fractions[*i] += to_be_distributed_per_fr + rest;
                let (clamped, rest_) = clamp_01(fractions[*i]);
                fractions[*i] = clamped;
                rest += rest_;
            }
        }

        if to_be_distributed_per_fr < 0.0 {
            update(
                sorted_indices(&fractions).iter(),
                pivot_idx,
                &mut fractions,
                to_be_distributed_per_fr,
                fixed,
            );
        } else {
            update(
                sorted_indices(&fractions).iter().rev(),
                pivot_idx,
                &mut fractions,
                to_be_distributed_per_fr,
                fixed,
            );
        }
        fractions
    }
}

fn add_fraction(mut fractions: Vec<f64>) -> Vec<f64> {
    let new_fraction = 1.0 / (1.0 + fractions.len() as f64);
    fractions.push(new_fraction);
    let pivot_idx = fractions.len() - 1;
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

type ComputeData<'a> = (Vec<&'a [f64]>, Vec<f64>, Vec<Vec<f64>>);

#[derive(Default, Clone, Debug)]
pub struct Charts {
    tmp: Chart,
    pub persisted: Vec<Chart>,
    fractions: Vec<f64>,
    fractions_fixed: Vec<bool>,
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
        let released = self.user_start.month_slider(ui, "begin");

        if self.user_start.is_at_end() {
            self.user_start.move_left();
        }
        while self.user_start.is_initialized()
            && self.user_end.selected_date() <= self.user_start.selected_date()
        {
            self.user_end.move_right();
        }
        released
    }
    pub fn end_slider(&mut self, ui: &mut Ui) -> bool {
        let released = self.user_end.month_slider(ui, "end");

        if self.user_end.is_at_start() {
            self.user_end.move_right();
        }
        while self.user_end.is_initialized()
            && self.user_end.selected_date() <= self.user_start.selected_date()
        {
            self.user_start.move_left();
        }
        released
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
            self.fractions_fixed.push(false);
        }
    }

    pub fn remove(&mut self, idx: usize) {
        self.persisted.remove(idx);
        self.fractions_fixed.remove(idx);
        let fr_removed = self.fractions.remove(idx);
        let new_fractions = redestribute_fractions(mem::take(&mut self.fractions), fr_removed);
        self.fractions = new_fractions;
    }

    pub fn fraction_sliders(&mut self, ui: &mut Ui) -> bool {
        let chart_inds = 0..(self.persisted.len());
        let mut remove_idx = None;
        let mut recompute = false;
        for idx in chart_inds {
            ui.label(self.persisted[idx].name());
            let slider = ui.add(egui::Slider::new(&mut self.fractions[idx], 0.0..=1.0));
            if slider.changed() {
                self.fractions =
                    normalize_fractions(mem::take(&mut self.fractions), idx, &self.fractions_fixed);
            }

            if slider.drag_released() {
                recompute = true;
            }

            if ui.button("x").clicked() {
                remove_idx = Some(idx);
            }
            ui.checkbox(&mut self.fractions_fixed[idx], "fix");
            ui.end_row();
        }
        if let Some(idx) = remove_idx {
            self.remove(idx);
        }
        recompute
    }

    fn gather_compute_data(
        &self,
        initial_balance: f64,
        monthly_payments: f64,
        start_date: Date,
        end_date: Date,
    ) -> BlcResult<ComputeData<'_>> {
        let price_devs = self
            .persisted
            .iter()
            .map(|c| c.sliced_values(start_date, end_date))
            .collect::<BlcResult<Vec<_>>>()?;
        let shortest_len = find_shortestlen(&price_devs)?;
        let initial_balances = self
            .fractions
            .iter()
            .map(|fr| fr * initial_balance)
            .collect::<Vec<_>>();
        let monthly_payments = self
            .fractions
            .iter()
            .map(|fr| vec![monthly_payments * *fr; shortest_len - 1])
            .collect::<Vec<_>>();
        Ok((price_devs, initial_balances, monthly_payments))
    }

    pub fn find_bestrebalancetrigger(
        &self,
        initial_balance: f64,
        monthly_payments: f64,
    ) -> BlcResult<BestRebalanceTrigger> {
        let (start_date, end_date) = self.start_end_date(false)?;
        let (price_devs, initial_balances, monthly_payments) =
            self.gather_compute_data(initial_balance, monthly_payments, start_date, end_date)?;
        let monthly_payments_refs = monthly_payments
            .iter()
            .map(|mp| &mp[..])
            .collect::<Vec<_>>();
        best_rebalance_trigger(&price_devs, &initial_balances, Some(&monthly_payments_refs))
    }
    pub fn compute_rebalancestats(
        &self,
        initial_balance: f64,
        monthly_payments: f64,
        rebalance_trigger: RebalanceTrigger,
    ) -> BlcResult<RebalanceStats> {
        let (start_date, end_date) = self.start_end_date(false)?;
        let (price_devs, initial_balances, monthly_payments) =
            self.gather_compute_data(initial_balance, monthly_payments, start_date, end_date)?;
        let monthly_payments_refs = monthly_payments
            .iter()
            .map(|mp| &mp[..])
            .collect::<Vec<_>>();
        rebalance_stats(
            &price_devs,
            &initial_balances,
            Some(&monthly_payments_refs),
            RebalanceData {
                trigger: rebalance_trigger,
                fractions: &self.fractions,
            },
            10,
        )
    }

    pub fn compute_balance(
        &mut self,
        initial_balance: f64,
        monthly_payments: f64,
        rebalance_trigger: RebalanceTrigger,
    ) -> BlcResult<()> {
        let (start_date, end_date) = self.start_end_date(false)?;
        let (price_devs, initial_balances, monthly_payments) =
            self.gather_compute_data(initial_balance, monthly_payments, start_date, end_date)?;
        let monthly_payments_refs = monthly_payments
            .iter()
            .map(|mp| &mp[..])
            .collect::<Vec<_>>();
        let balance_over_month = compute_balance_over_months(
            &price_devs,
            &initial_balances,
            Some(&monthly_payments_refs),
            RebalanceData {
                trigger: rebalance_trigger,
                fractions: &self.fractions,
            },
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

#[test]
fn test_adaptfractions() {
    fn test(input: Vec<f64>, reference: Vec<f64>, idx: usize, fixed: &[bool]) {
        let fixed = if fixed.is_empty() {
            &[false, false, false]
        } else {
            fixed
        };
        let result = normalize_fractions(input, idx, fixed);
        assert!(!result.is_empty());
        for (res, refe) in result.iter().zip(reference.iter()) {
            assert!((res - refe).abs() < 1e-12);
        }
    }
    test(vec![0.1, 0.3], vec![0.7, 0.3], 0, &[false, true]);
    test(
        vec![0.1, 0.3, 0.9],
        vec![0.0, 0.3, 0.7],
        2,
        &[false, true, false],
    );
    test(
        vec![0.1, 0.1, 0.9],
        vec![0.0, 0.1, 0.9],
        2,
        &[false, true, false],
    );
    test(
        vec![-1.9, 0.3, 0.1],
        vec![0.0, 0.6, 0.4],
        0,
        &[true, false, false],
    );
    test(vec![0.9, 0.05, 0.5], vec![0.5, 0.0, 0.5], 2, &[]);
    test(
        vec![0.1, 0.1, 0.5],
        vec![0.4, 0.1, 0.5],
        2,
        &[false, true, false],
    );
    test(vec![0.1, 0.1, 0.5], vec![0.25, 0.25, 0.5], 2, &[]);
    test(vec![0.2, 0.1, 0.1], vec![0.2, 0.4, 0.4], 0, &[]);
    test(vec![0.9, 0.1, 0.1], vec![0.9, 0.05, 0.05], 0, &[]);
    test(vec![1.9, 0.1, 0.1], vec![1.0, 0.0, 0.0], 0, &[]);
    test(vec![-1.9, 0.3, 0.1], vec![0.0, 0.6, 0.4], 0, &[]);
}
