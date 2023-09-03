use crate::{
    blcerr,
    // charts::MonthlyPayments,
    core_types::{to_blc, BlcError, BlcResult},
    date::{Date, Interval},
};
use exmex::{Express, FlatExVal, Val};
use rand::{rngs::StdRng, SeedableRng};
use rand_distr::{Distribution, Normal};
use serde::{Deserialize, Serialize};
use std::iter;

pub type Expr = FlatExVal<i32, f64>;

fn eval(expr: &Expr, vars: &[Val<i32, f64>]) -> BlcResult<f64> {
    let evaluated = expr.eval_relaxed(vars).map_err(to_blc)?;
    let x = match evaluated {
        Val::Float(x) => x,
        Val::Int(n) => n as f64,
        Val::Bool(b) => {
            if b {
                1.0
            } else {
                0.0
            }
        }
        Val::None => Err(blcerr!("Parsed expression returned none"))?,
        Val::Error(e) => Err(BlcError::new(e.msg()))?,
    };
    Ok(x)
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyPayments {
    // payment per interval, the expression can evaluate the variables current_balance and
    // initial_balance.
    payments: Vec<Expr>,
    intervals: Vec<Option<Interval>>,
}
impl MonthlyPayments {
    pub fn from_intervals(payments: Vec<Expr>, intervals: Vec<Interval>) -> BlcResult<Self> {
        if payments.len() != intervals.len() {
            Err(blcerr!("payments and intervals need to be equally long"))
        } else {
            Ok(MonthlyPayments {
                payments,
                intervals: intervals.into_iter().map(Some).collect(),
            })
        }
    }
    pub fn from_single_payment(payment: Expr) -> Self {
        MonthlyPayments {
            payments: vec![payment],
            intervals: vec![None],
        }
    }
    /// Computes all payments of the current_date
    pub fn compute(&self, current_date: Date, vars: &[Val<i32, f64>]) -> BlcResult<f64> {
        self.payments
            .iter()
            .zip(self.intervals.iter())
            .filter(|(_, inter)| {
                if let Some(inter) = inter {
                    inter.contains(current_date)
                } else {
                    true
                }
            })
            .map(|(pay, _)| eval(pay, vars))
            .try_fold::<f64, _, _>(0.0, |x, y| y.map(|y| x + y))
    }
}
pub fn yearly_return(total_payments: f64, n_months: usize, final_balance: f64) -> (f64, f64) {
    let total_yield = final_balance / total_payments;
    if total_payments < 0.0 {
        (f64::NAN, total_yield)
    } else {
        let yearly_return_perc =
            100.0 * (total_yield.powf(1.0 / ((n_months - 1) as f64 / 12.0)) - 1.0);
        (yearly_return_perc, total_yield)
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct RebalanceTrigger {
    pub interval: Option<usize>,
    pub deviation: Option<f64>,
}
impl RebalanceTrigger {
    fn from_both(interval: usize, deviation: f64) -> Self {
        RebalanceTrigger {
            interval: Some(interval),
            deviation: Some(deviation),
        }
    }
    fn from_interval(interval: usize) -> Self {
        RebalanceTrigger {
            interval: Some(interval),
            deviation: None,
        }
    }
    fn from_dev(deviation: f64) -> Self {
        RebalanceTrigger {
            interval: None,
            deviation: Some(deviation),
        }
    }
}

impl<'a> RebalanceData<'a> {
    fn is_triggered_by_interval(&self, month: usize) -> bool {
        if let Some(interval) = self.trigger.interval {
            interval > 0 && month % interval == 0
        } else {
            false
        }
    }
    fn is_triggered_by_deviation(&self, balances: &[f64]) -> bool {
        if let Some(max_dev) = self.trigger.deviation {
            let total_balance = balances.iter().sum::<f64>();
            let deviation = balances
                .iter()
                .zip(self.fractions)
                .map(|(b, fr)| ((fr - b / total_balance).abs()))
                .max_by(|a, b| a.partial_cmp(b).unwrap());
            deviation > Some(max_dev)
        } else {
            false
        }
    }
    pub fn is_triggered(&self, balances: &[f64], month: usize) -> bool {
        if self.trigger.interval.is_some() && self.trigger.deviation.is_some() {
            self.is_triggered_by_interval(month) && self.is_triggered_by_deviation(balances)
        } else {
            self.is_triggered_by_interval(month) || self.is_triggered_by_deviation(balances)
        }
    }
}
#[derive(Clone, Debug)]
pub struct RebalanceData<'a> {
    /// after how many months is re-balancing applied
    pub trigger: RebalanceTrigger,
    /// fractions of the indices
    pub fractions: &'a [f64],
}
impl<'a> RebalanceData<'a> {
    fn wo_trigger(other: Self) -> Self {
        Self {
            trigger: RebalanceTrigger {
                interval: None,
                deviation: None,
            },
            fractions: other.fractions,
        }
    }
    fn from_fractions(fractions: &'a [f64]) -> Self {
        Self {
            trigger: RebalanceTrigger {
                interval: None,
                deviation: None,
            },
            fractions,
        }
    }
}

pub fn find_shortestlen<'a>(price_devs: &'a [&'a [f64]]) -> Option<usize> {
    price_devs.iter().map(|pd| pd.len()).min()
}

///
/// Compute the balance given initial values and price developments of securities
///
/// Arguments
/// * `price_devs`         - developments of the individual securities (e.g., stock prices, index prices, ...)
///                          2d-vector, first axis addresses the security, second axis is the price
/// * `initial_balance`    - total amount of initial investment
/// * `monthly_payments    - monthly payments for each security, e.g., from a savings plan
/// * `rebalance_interval` - pass if indices are rebalanced
/// * `start_date`         - needed to check if which monthly payments are due
///
/// Returns an iterator that yields total balance and the sum of all payments per months up to each month
///
pub fn compute_balance_over_months<'a>(
    price_devs: &'a [&'a [f64]],
    initial_balance: f64,
    monthly_payments: Option<&'a MonthlyPayments>,
    rebalance_data: RebalanceData<'a>,
    start_date: Date,
) -> impl Iterator<Item = BlcResult<(f64, f64)>> + 'a {
    let initial_balances = rebalance_data
        .fractions
        .iter()
        .map(|fr| fr * initial_balance)
        .collect::<Vec<f64>>();
    let shortest_len = find_shortestlen(price_devs).unwrap_or(0);
    let balances_over_months = (0..shortest_len).zip(1..shortest_len).scan(
        (initial_balances, 0.0),
        move |(balances, monthly_payments_upto_now), (i_prev_month, i_month)| {
            // immediately called closure for error handling,
            // since outer closure has to return Option
            let res = (|| {
                let fractions = &rebalance_data.fractions;
                for i_security in 0..balances.len() {
                    let vars = vec![
                        Val::Float(balances.iter().sum::<f64>()),
                        Val::Float(initial_balance),
                    ];
                    let payment_this_month = monthly_payments
                        .map(|mp| mp.compute((start_date + i_month)?, &vars))
                        .unwrap_or(Ok(0.0))?;
                    // we assume the monthly payment at the beggining of the month
                    let price_update = (payment_this_month * fractions[i_security]
                        + balances[i_security])
                        * price_devs[i_security][i_month]
                        / price_devs[i_security][i_prev_month];
                    balances[i_security] = price_update;
                    *monthly_payments_upto_now += payment_this_month;
                }

                let total: f64 = balances.iter().sum();
                if rebalance_data.is_triggered(balances, i_month) {
                    rebalance_data
                        .fractions
                        .iter()
                        .zip(balances.iter_mut())
                        .for_each(|(frac, balance)| {
                            *balance = frac * total;
                        });
                }
                Ok((
                    balances.iter().sum::<f64>(),
                    initial_balance + *monthly_payments_upto_now,
                ))
            })();
            Some(res)
        },
    );
    iter::once(Ok((initial_balance, initial_balance))).chain(balances_over_months)
}

pub fn unzip_balance_iter(
    balance_over_month: impl Iterator<Item = BlcResult<(f64, f64)>>,
) -> BlcResult<(Vec<f64>, Vec<f64>)> {
    let mut balances = vec![];
    let mut payments = vec![];
    for bom in balance_over_month {
        let (b, p) = bom?;
        balances.push(b);
        payments.push(p);
    }
    Ok((balances, payments))
}

#[allow(clippy::needless_lifetimes)]
pub fn adapt_pricedev_to_initial_balance<'a>(
    initial_balance: f64,
    price_dev: &'a [f64],
) -> impl Iterator<Item = f64> + 'a {
    let mut balance = initial_balance;
    iter::once(initial_balance).chain(
        price_dev[0..price_dev.len()]
            .iter()
            .zip(price_dev[1..].iter())
            .map(move |(pd_prev, pd)| {
                balance = balance * pd / pd_prev;
                balance
            }),
    )
}

#[cfg(target_arch = "wasm32")]
pub fn unix_to_now_nanos() -> BlcResult<u64> {
    let now = (js_sys::Date::now() * 1000.0) as u128;
    Ok((now % (u64::MAX as u128)) as u64)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn unix_to_now_nanos() -> BlcResult<u64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    Ok((SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(to_blc)?
        .as_nanos()
        % (u64::MAX as u128)) as u64)
}

pub fn random_walk(
    expected_yearly_return: f64,
    is_markovian: bool,
    sigma_mean: f64,
    sigma_window_size: usize,
    n_months: usize,
    crashes: &[usize],
) -> BlcResult<Vec<f64>> {
    let mut rng = StdRng::seed_from_u64(unix_to_now_nanos()?);
    let mut sigma_rng = StdRng::seed_from_u64(unix_to_now_nanos()?);
    let sigma_distribution = Normal::new(sigma_mean, sigma_mean).map_err(to_blc)?;
    let mut last_sigmas = vec![sigma_mean; sigma_window_size];
    let start_price = 1e5;
    let mut res = vec![start_price; n_months + 1];
    let expected_monthly_return = (1.0 + (expected_yearly_return / 100.0)).powf(1.0 / 12.0);
    let mut mu = expected_monthly_return;
    let crash_radius = 3;

    let crash_mu_dist_factors = (0..crash_radius)
        .map(|distance| 0.7 + 0.3 * distance as f64 / crash_radius as f64)
        .collect::<Vec<_>>();
    let crash_mu_factors = (0..n_months)
        .map(|m| {
            let d = crashes
                .iter()
                .map(|c| (m as i32 - *c as i32).abs())
                .min()
                .unwrap_or(n_months as i32) as usize;
            if d < crash_radius {
                crash_mu_dist_factors[d]
            } else {
                1.0
            }
        })
        .collect::<Vec<_>>();
    for (i, sigma) in (1..(n_months + 1)).zip(sigma_distribution.sample_iter(&mut sigma_rng)) {
        for i in 0..(sigma_window_size - 1) {
            last_sigmas[i] = last_sigmas[i + 1];
        }
        last_sigmas[sigma_window_size - 1] = sigma;
        last_sigmas.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let sigma = last_sigmas[sigma_window_size / 2].abs();
        let d = Normal::new(mu * crash_mu_factors[i - 1], sigma).map_err(to_blc)?;
        let monthly_factor = d.sample(&mut rng);
        res[i] = res[i - 1] * monthly_factor;

        if !is_markovian && sigma - sigma_mean > 0.0 {
            let actual_total_return: f64 = (1..=i)
                .map(|j| res[j] / res[j - 1])
                .product::<f64>()
                .powf(1.0 / (n_months - i) as f64);
            let expected_total_return =
                expected_monthly_return.powf(n_months as f64 / (n_months - i) as f64);
            mu = expected_total_return / actual_total_return;
        }
    }
    Ok(res)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RebalanceStatRecord {
    pub mean_w_reb: f64,
    pub mean_wo_reb: f64,
    pub n_months: usize,
}

fn compute_mean(
    records: &[RebalanceStatRecord],
    f: impl Fn(&RebalanceStatRecord) -> f64,
    begin: usize,
    end: usize,
) -> f64 {
    let s = (begin..end).map(|i| f(&records[i])).sum::<f64>();
    s / (end - begin) as f64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceStats {
    pub records: Vec<RebalanceStatRecord>,
}
impl RebalanceStats {
    pub fn mean_across_nmonths(&self) -> BlcResult<RebalanceStatsSummary> {
        let min_n_months = self
            .records
            .iter()
            .map(|r| r.n_months)
            .min()
            .ok_or_else(|| blcerr!("no records found"))?;
        let max_n_months = self.records.iter().map(|r| r.n_months).max().unwrap();

        let len_records = self.records.len();

        let n_33 = (len_records as f64 * 0.33).round() as usize;
        let n_67 = (len_records as f64 * 0.67).round() as usize;

        let mean_across_months_w_reb_min_33 =
            compute_mean(&self.records, |r| r.mean_w_reb, 0, n_33);
        let mean_across_months_wo_reb_min_33 =
            compute_mean(&self.records, |r| r.mean_wo_reb, 0, n_33);
        let mean_across_months_w_reb_33_67 =
            compute_mean(&self.records, |r| r.mean_w_reb, n_33, n_67);
        let mean_across_months_wo_reb_33_67 =
            compute_mean(&self.records, |r| r.mean_wo_reb, n_33, n_67);
        let mean_across_months_w_reb_67_max =
            compute_mean(&self.records, |r| r.mean_w_reb, n_67, len_records);
        let mean_across_months_wo_reb_67_max =
            compute_mean(&self.records, |r| r.mean_wo_reb, n_67, len_records);

        let mean_across_months_w_reb =
            compute_mean(&self.records, |r| r.mean_w_reb, 0, len_records);
        let mean_across_months_wo_reb =
            compute_mean(&self.records, |r| r.mean_wo_reb, 0, len_records);

        Ok(RebalanceStatsSummary {
            min_n_months,
            max_n_months,
            n_months_33: self.records[n_33].n_months,
            n_months_67: self.records[n_67].n_months,
            mean_across_months_w_reb,
            mean_across_months_wo_reb,
            mean_across_months_w_reb_min_33,
            mean_across_months_wo_reb_min_33,
            mean_across_months_w_reb_33_67,
            mean_across_months_wo_reb_33_67,
            mean_across_months_w_reb_67_max,
            mean_across_months_wo_reb_67_max,
        })
    }
}

#[derive(Deserialize, Serialize)]
pub struct RebalanceStatsSummary {
    pub min_n_months: usize,
    pub max_n_months: usize,
    pub n_months_33: usize,
    pub n_months_67: usize,
    pub mean_across_months_w_reb: f64,
    pub mean_across_months_wo_reb: f64,
    pub mean_across_months_w_reb_min_33: f64,
    pub mean_across_months_wo_reb_min_33: f64,
    pub mean_across_months_w_reb_33_67: f64,
    pub mean_across_months_wo_reb_33_67: f64,
    pub mean_across_months_w_reb_67_max: f64,
    pub mean_across_months_wo_reb_67_max: f64,
}

pub fn rebalance_stats<'a>(
    price_devs: &'a [&'a [f64]],
    initial_balance: f64,
    monthly_payments: Option<&'a MonthlyPayments>,
    rebalance_data: RebalanceData<'a>,
    start_date: Date,
    min_n_months: usize,
) -> BlcResult<RebalanceStats> {
    let shortest_len = find_shortestlen(price_devs)
        .ok_or_else(|| BlcError::new("no price-devs, no rebalance stats"))?;
    let comp_bal = |start_idx: usize, n_months: usize, data: RebalanceData<'a>| {
        let price_devs_cur: Vec<&[f64]> = price_devs
            .iter()
            .map(|pd| &pd[start_idx..(start_idx + n_months)])
            .collect();
        let (balance, _) = compute_total_balance(
            &price_devs_cur,
            initial_balance,
            monthly_payments,
            data,
            start_date,
        )?;
        Ok(balance)
    };
    let records = (min_n_months..shortest_len + 1)
        .map(|n_months| -> BlcResult<RebalanceStatRecord> {
            let last_start_month = shortest_len - n_months + 1;
            let bsum_w_reb: f64 = (0..last_start_month)
                .map(|start_idx| comp_bal(start_idx, n_months, rebalance_data.clone()))
                .try_fold::<f64, _, _>(0.0, |x, y: Result<f64, BlcError>| y.map(|y| x + y))?;
            let bsum_wo_reb: f64 = (0..last_start_month)
                .map(|start_idx| {
                    comp_bal(
                        start_idx,
                        n_months,
                        RebalanceData::wo_trigger(rebalance_data.clone()),
                    )
                })
                .try_fold::<f64, _, _>(0.0, |x, y| y.map(|y| x + y))?;
            let mean_w_reb = bsum_w_reb / last_start_month as f64;
            let mean_wo_reb = bsum_wo_reb / last_start_month as f64;
            Ok(RebalanceStatRecord {
                mean_w_reb,
                mean_wo_reb,
                n_months,
            })
        })
        .collect::<BlcResult<Vec<_>>>()?;
    Ok(RebalanceStats { records })
}

#[derive(Deserialize, Serialize)]
pub struct BestRebalanceTrigger {
    pub best: (RebalanceTrigger, f64, f64),
    pub with_best_dev: (RebalanceTrigger, f64, f64),
    pub with_best_interval: (RebalanceTrigger, f64, f64),
}

pub fn best_rebalance_trigger(
    price_devs: &[&[f64]],
    initial_balance: f64,
    monthly_payments: Option<&MonthlyPayments>,
    fractions: &[f64],
    start_date: Date,
) -> BlcResult<BestRebalanceTrigger> {
    let shortest_len =
        find_shortestlen(price_devs).ok_or_else(|| BlcError::new("empty price dev"))?;
    let months_to_test = 0..(shortest_len / 2);
    let deviations_to_test = (0..10).chain((20..50).step_by(10)).chain(iter::once(75));
    let triggers: Vec<(RebalanceTrigger, f64, f64)> = months_to_test
        .flat_map(move |n_months| {
            iter::repeat(n_months).zip(deviations_to_test.clone()).map(
                move |(n_months, d)| -> BlcResult<_> {
                    let rebalance_data = if n_months == 0 && d == 0 {
                        RebalanceData::from_fractions(fractions)
                    } else {
                        let trigger = if n_months == 0 {
                            RebalanceTrigger::from_dev(d as f64 / 100.0)
                        } else if d == 0 {
                            RebalanceTrigger::from_interval(n_months)
                        } else {
                            RebalanceTrigger::from_both(n_months, d as f64 / 100.0)
                        };
                        RebalanceData { trigger, fractions }
                    };
                    let trigger = rebalance_data.trigger;
                    let (balance, total_payments) = compute_total_balance(
                        price_devs,
                        initial_balance,
                        monthly_payments,
                        rebalance_data,
                        start_date,
                    )?;
                    Ok((trigger, balance, total_payments))
                },
            )
        })
        .collect::<BlcResult<Vec<_>>>()?;
    let (best_trigger, best_balance, _) = triggers
        .iter()
        .max_by(|(_, a, _), (_, b, _)| a.partial_cmp(b).unwrap())
        .ok_or(blcerr!("could not find best trigger"))?;
    let (best_dev, best_dev_balance, _) = triggers
        .iter()
        .filter(|(t, _, _)| t.interval.is_none())
        .max_by(|(_, a, _), (_, b, _)| a.partial_cmp(b).unwrap())
        .ok_or(blcerr!("could not find best trigger"))?;
    let (best_interval, best_interval_balance, total_payments) = triggers
        .iter()
        .filter(|(t, _, _)| t.deviation.is_none())
        .max_by(|(_, a, _), (_, b, _)| a.partial_cmp(b).unwrap())
        .ok_or(blcerr!("could not find best trigger"))?;

    Ok(BestRebalanceTrigger {
        best: (*best_trigger, *best_balance, *total_payments),
        with_best_dev: (*best_dev, *best_dev_balance, *total_payments),
        with_best_interval: (*best_interval, *best_interval_balance, *total_payments),
    })
}

fn compute_total_balance(
    price_devs: &[&[f64]],
    initial_balance: f64,
    monthly_payments: Option<&MonthlyPayments>,
    rebalance_data: RebalanceData<'_>,
    start_date: Date,
) -> BlcResult<(f64, f64)> {
    compute_balance_over_months(
        price_devs,
        initial_balance,
        monthly_payments,
        rebalance_data,
        start_date,
    )
    .last()
    .unwrap()
}

#[cfg(test)]
use exmex::parse_val;

#[test]
fn test_adapt() {
    let price_dev = [3.0, 6.0, 12.0, 6.0];
    let price_ref = [10.0, 20.0, 40.0, 20.0];
    let adapted = adapt_pricedev_to_initial_balance(10.0, &price_dev);
    for (a, p) in adapted.zip(price_ref.iter()) {
        assert!((a - p) < 1e-12);
    }
}

#[test]
fn test_compute_balance() {
    let rebalance_interval = 5;
    let world_vals = iter::repeat(1.0)
        .take(rebalance_interval)
        .chain(iter::repeat(2.0).take(rebalance_interval))
        .chain(iter::repeat(4.0).take(rebalance_interval))
        .collect::<Vec<_>>();
    let em_vals = vec![1.0; rebalance_interval * 3];
    let d202005 = Date::new(2020, 5).unwrap();
    let (b, p) = compute_total_balance(
        &[&world_vals, &em_vals],
        1.0,
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: Some(rebalance_interval),
                deviation: None,
            },
            fractions: &[0.5, 0.5],
        },
        d202005,
    )
    .unwrap();
    assert!((b - 2.25).abs() < 1e-12);
    assert!((p - 1.0).abs() < 1e-12);

    let (b, p) = compute_total_balance(
        &[&world_vals, &em_vals],
        10.0,
        None,
        RebalanceData::from_fractions(&[0.7, 0.3]),
        d202005,
    )
    .unwrap();
    assert!((b - 31.0).abs() < 1e-12);
    assert!((p - 10.0).abs() < 1e-12);

    let (x, p) = compute_total_balance(
        &[&world_vals, &em_vals],
        1.0,
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: Some(rebalance_interval),
                deviation: None,
            },
            fractions: &[0.7, 0.3],
        },
        d202005,
    )
    .unwrap();
    assert!((x - 2.89).abs() < 1e-12);
    assert!((p - 1.0).abs() < 1e-12);

    let (x, p) = compute_total_balance(
        &[&world_vals, &em_vals],
        1.0,
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: Some(rebalance_interval),
                deviation: None,
            },
            fractions: &[1.0, 0.0],
        },
        d202005,
    )
    .unwrap();
    assert!((x - 4.0).abs() < 1e-12);
    assert!((p - 1.0).abs() < 1e-12);

    let world_vals = vec![1.0; 24];
    let em_vals = vec![1.0; 24];
    let (x, p) = compute_total_balance(
        &[&world_vals, &em_vals],
        1.0,
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: Some(rebalance_interval),
                deviation: None,
            },
            fractions: &[0.7, 0.3],
        },
        d202005,
    )
    .unwrap();
    assert!((x - 1.0).abs() < 1e-12);
    assert!((p - 1.0).abs() < 1e-12);

    let world_vals = iter::repeat(1.0)
        .take(10)
        .chain(iter::once(1.1))
        .collect::<Vec<_>>();
    let em_vals = iter::repeat(1.0)
        .take(10)
        .chain(iter::once(1.1))
        .collect::<Vec<_>>();
    let (x, p) = compute_total_balance(
        &[&world_vals, &em_vals],
        1.0,
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: Some(11),
                deviation: None,
            },
            fractions: &[0.7, 0.3],
        },
        d202005,
    )
    .unwrap();
    assert!((x - 1.1).abs() < 1e-12);
    assert!((p - 1.0).abs() < 1e-12);
}

#[test]
fn test_compound() {
    let d202005 = Date::new(2020, 5).unwrap();
    let compound_interest: Vec<f64> = random_walk(5.0, true, 0.0, 12, 240, &[]).unwrap();
    let mp = MonthlyPayments::from_single_payment(parse_val("0").unwrap());
    let (b, p) = compute_total_balance(
        &[&compound_interest],
        10000.0,
        Some(&mp),
        RebalanceData::from_fractions(&[1.0]),
        d202005,
    )
    .unwrap();
    assert!((b - 26532.98).abs() < 1e-2);
    assert!((p - 10000.0).abs() < 1e-12);

    let compound_interest: Vec<f64> = random_walk(5.0, true, 0.0, 12, 360, &[]).unwrap();
    let monthly_payments = MonthlyPayments::from_single_payment(parse_val("1000.0").unwrap());
    let (b, _) = compute_total_balance(
        &[&compound_interest],
        10000.0,
        Some(&monthly_payments),
        RebalanceData::from_fractions(&[1.0]),
        d202005,
    )
    .unwrap();
    println!("{b}");
    assert!((b - 861917.27).abs() < 1e-2);

    let compound_interest: Vec<f64> = random_walk(5.0, true, 1.0, 12, 137, &[]).unwrap();
    let monthly_payments = MonthlyPayments::from_single_payment(parse_val("0.0").unwrap());
    let (_, total_p) = compute_total_balance(
        &[&compound_interest],
        10000.0,
        Some(&monthly_payments),
        RebalanceData::from_fractions(&[1.0]),
        d202005,
    )
    .unwrap();
    println!("total p {total_p}");
    assert!((total_p - 10000.0).abs() < 1e-12);

    let compound_interest: Vec<f64> = random_walk(5.0, true, 1.0, 12, 36, &[]).unwrap();
    let monthly_payments = MonthlyPayments::from_single_payment(parse_val("1000.0").unwrap());
    let (_, total_p) = compute_total_balance(
        &[&compound_interest],
        10000.0,
        Some(&monthly_payments),
        RebalanceData::from_fractions(&[1.0]),
        d202005,
    )
    .unwrap();
    println!("total p {total_p}");
    assert!((total_p - 46000.0).abs() < 1e-12);
}

#[test]
fn test_rebalance() {
    let d202005 = Date::new(2020, 5).unwrap();
    let v1s = vec![1.0, 1.0, 0.0];
    let v2s = vec![1.0, 1.0, 1.0];
    let pd = [v1s.as_slice(), v2s.as_slice()];
    let bom = compute_balance_over_months(
        &pd,
        1.0,
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: Some(1),
                deviation: None,
            },
            fractions: &[0.5, 0.5],
        },
        d202005,
    );
    let (x, _) = unzip_balance_iter(bom).unwrap();
    assert!((x[2] - 0.5).abs() < 1e-12);

    let v1s = vec![1.0, 1.0, 1.0];
    let v2s = vec![1.0, 0.5, 1.0];
    let pd = [v1s.as_slice(), v2s.as_slice()];
    let bom = compute_balance_over_months(
        &pd,
        1.0,
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: None,
                deviation: Some(0.1),
            },
            fractions: &[0.5, 0.5],
        },
        d202005,
    );
    let (x, _) = unzip_balance_iter(bom).unwrap();
    assert!((x[2] - 1.125).abs() < 1e-12);
}

#[test]
fn test_besttrigger() {
    let d202005 = Date::new(2020, 5).unwrap();
    let v1s = vec![1.0, 1.0, 1.0, 1.0, 0.5, 1.0];
    let v2s = vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    let (_, balance, _) = best_rebalance_trigger(&[&v1s, &v2s], 1.0, None, &[0.5, 0.5], d202005)
        .unwrap()
        .best;
    assert!((balance - 1.125).abs() < 1e-12);
}
#[test]
fn test_rebalancestats() {
    let d202005 = Date::new(2020, 5).unwrap();
    let v1s = vec![1.0, 1.0, 1.0, 1.0, 0.5, 1.0];
    let v2s = vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    let min_n_months = 3;
    let stats = rebalance_stats(
        &[&v1s, &v2s],
        1.0,
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: Some(1),
                deviation: None,
            },
            fractions: &[0.5, 0.5],
        },
        d202005,
        min_n_months,
    )
    .unwrap();
    assert!(stats.records.len() == min_n_months + 1);
    let ref_means_wo = [0.9375, 0.9166666666666666, 0.875, 1.0];
    let ref_means_w = [0.96875, 0.9583333333333334, 0.9375, 1.125];
    for (i, r) in stats.records.iter().enumerate() {
        let n_months = i + min_n_months;
        assert_eq!(r.n_months, n_months);
        assert!((r.mean_wo_reb - ref_means_wo[i]).abs() < 1e-6);
        assert!((r.mean_w_reb - ref_means_w[i]).abs() < 1e-6);
    }

    let stat0 = RebalanceStatRecord {
        mean_w_reb: 4.0,
        mean_wo_reb: 2.0,
        n_months: 4,
    };
    let stat1 = RebalanceStatRecord {
        mean_w_reb: 2.0,
        mean_wo_reb: 1.0,
        n_months: 3,
    };
    let stats = RebalanceStats {
        records: vec![stat0, stat1],
    };
    let stats_summary = stats.mean_across_nmonths().unwrap();
    assert!((stats_summary.mean_across_months_w_reb - 3.0).abs() < 1e-12);
    assert!((stats_summary.mean_across_months_wo_reb - 1.5).abs() < 1e-12);
}

#[test]
fn test_monthly() {
    let d1 = Date::new(2000, 11).unwrap();
    let mp = MonthlyPayments::from_single_payment(parse_val("cb / ib").unwrap());
    let vars = &[Val::Float(2.0), Val::Float(2.9)];
    let res = mp.compute(d1, vars).unwrap();
    assert!((res - 2.0 / 2.9).abs() < 1e-9);
    let expr1 = parse_val("1.0 / cb").unwrap();
    let expr2 = parse_val("7.0").unwrap();
    let payments = vec![expr1, expr2];
    let d2 = Date::new(2013, 11).unwrap();
    let d3 = Date::new(2012, 11).unwrap();
    let d4 = Date::new(2014, 11).unwrap();
    let intervals = vec![
        Interval::new(d1, d2).unwrap(),
        Interval::new(d3, d4).unwrap(),
    ];
    let mp = MonthlyPayments::from_intervals(payments, intervals).unwrap();
    let res = mp.compute(Date::new(2000, 10).unwrap(), vars).unwrap();
    assert!(res.abs() < 1e-9);
    let res = mp.compute(Date::new(2001, 10).unwrap(), vars).unwrap();
    assert!((res - 0.5).abs() < 1e-9);
    let res = mp.compute(Date::new(2007, 10).unwrap(), vars).unwrap();
    assert!((res - 0.5).abs() < 1e-9);
    let res = mp.compute(Date::new(2013, 10).unwrap(), vars).unwrap();
    assert!((res - 7.5).abs() < 1e-9);
}
