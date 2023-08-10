use crate::{
    blcerr,
    core_types::{to_blc, BlcResult},
};
use rand::{rngs::StdRng, SeedableRng};
use rand_distr::{Distribution, Normal};
use std::iter;

pub fn yearly_return(
    initial_payment: f64,
    monthly_payment: f64,
    n_months: usize,
    final_balance: f64,
) -> (f64, f64) {
    let total_monthly = monthly_payment * (n_months - 1) as f64;
    let total_yield = final_balance / (initial_payment + total_monthly);
    let yearly_return_perc = 100.0 * (total_yield.powf(1.0 / ((n_months - 1) as f64 / 12.0)) - 1.0);
    (yearly_return_perc, total_yield)
}

#[derive(Clone, Copy, Debug, Default)]
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

pub fn find_shortestlen<'a>(price_devs: &'a [&'a [f64]]) -> BlcResult<usize> {
    price_devs
        .iter()
        .map(|pd| pd.len())
        .min()
        .ok_or(blcerr!("empty price dev"))
}

fn balances_to_fractions(balances: &[f64]) -> Vec<f64> {
    let total: f64 = balances.iter().sum();
    balances.iter().map(|b| b / total).collect()
}

///
/// Compute the balance given initial values and price developments of securities
///
/// Arguments
/// * `price_devs` - developments of the individual securities (e.g., stock prices, index prices, ...)
///                  2d-vector, first axis addresses the security, second axis is the price
/// * `initial_balances` - initial balance per security (e.g., stock price, index price, ...)
/// * `monthly_payments - monthly payments for each security, e.g., from a savings plan
///                       index 0 here corresponds to index 1 in price dev, since the month-0-payment
///                       is covered by `initial_balances`
/// * `rebalance_interval` - pass if indices are rebalanced
///
/// Returns an iterator that yields total balance and the sum of all payments per months up to each month
///
pub fn compute_balance_over_months<'a>(
    price_devs: &'a [&'a [f64]],
    initial_balances: &'a [f64],
    monthly_payments: Option<&'a [&'a [f64]]>,
    rebalance_data: RebalanceData<'a>,
) -> BlcResult<impl Iterator<Item = (f64, f64)> + 'a> {
    let total_initial_balances: f64 = initial_balances.iter().sum();
    let shortest_len = find_shortestlen(price_devs)?;
    let balances_over_months = (0..shortest_len).zip(1..shortest_len).scan(
        (initial_balances.to_vec(), 0.0),
        move |(balances, monthly_payments_upto_now), (i_prev_month, i_month)| {
            // update the balance for each security at the current month
            for i_sec in 0..balances.len() {
                let payment_this_month = monthly_payments
                    .map(|mp| mp[i_sec][i_month - 1])
                    .unwrap_or(0.0);
                // we assume the monthly payment at the beggining of the month
                let price_update = (payment_this_month + balances[i_sec])
                    * price_devs[i_sec][i_month]
                    / price_devs[i_sec][i_prev_month];
                balances[i_sec] = price_update;
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
            Some((
                balances.iter().sum::<f64>(),
                total_initial_balances + *monthly_payments_upto_now,
            ))
        },
    );
    Ok(iter::once((total_initial_balances, total_initial_balances)).chain(balances_over_months))
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
    use wasm_bindgen::prelude::*;
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

const SIGMA_WINDOW_SIZE: usize = 12;

pub fn random_walk(
    expected_yearly_return: f64,
    is_return_indpendent: bool,
    sigma_mean: f64,
    n_months: usize,
) -> BlcResult<Vec<f64>> {
    let mut sigma_rng = StdRng::seed_from_u64(unix_to_now_nanos()?);
    let sigma_distribution = Normal::new(sigma_mean, sigma_mean).map_err(to_blc)?;
    let mut last_sigmas = [sigma_mean; SIGMA_WINDOW_SIZE];
    let mut rv_rng = StdRng::seed_from_u64(unix_to_now_nanos()?);
    let start_price = 1.0;
    let mut res = vec![start_price; n_months + 1];
    let expected_monthly_return = (1.0 + (expected_yearly_return / 100.0)).powf(1.0 / 12.0);
    let mut mu = expected_monthly_return;
    for (i, sigma) in (1..(n_months + 1)).zip(sigma_distribution.sample_iter(&mut sigma_rng)) {
        for i in 0..9 {
            last_sigmas[i] = last_sigmas[i + 1];
        }
        last_sigmas[9] = sigma;
        last_sigmas.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let sigma = last_sigmas[SIGMA_WINDOW_SIZE / 2].abs();
        let d = Normal::new(mu, sigma).map_err(to_blc)?;
        let rv = d.sample(&mut rv_rng);
        res[i] = (res[i - 1] * rv).max(1e-1);

        if !is_return_indpendent && sigma - sigma_mean > 0.0 {
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

const NONE_REBALANCE_DATA: RebalanceData<'_> = RebalanceData {
    trigger: RebalanceTrigger {
        interval: None,
        deviation: None,
    },
    fractions: &[],
};
pub fn rebalance_stats<'a>(
    price_devs: &'a [&'a [f64]],
    initial_balances: &'a [f64],
    monthly_payments: Option<&'a [&'a [f64]]>,
    rebalance_data: RebalanceData<'a>,
    min_n_months: usize,
) -> BlcResult<RebalanceStats> {
    let shortest_len = find_shortestlen(price_devs)?;
    let comp_bal = |start_idx: usize, n_months: usize, data: RebalanceData<'a>| {
        let price_devs_cur: Vec<&[f64]> = price_devs
            .iter()
            .map(|pd| &pd[start_idx..(start_idx + n_months)])
            .collect();
        let (balance, _) =
            compute_total_balance(&price_devs_cur, initial_balances, monthly_payments, data);
        balance
    };
    let records = (min_n_months..shortest_len + 1)
        .map(|n_months| {
            let last_start_month = shortest_len - n_months + 1;
            let bsum_w_reb: f64 = (0..last_start_month)
                .map(|start_idx| comp_bal(start_idx, n_months, rebalance_data.clone()))
                .sum();
            let bsum_wo_reb: f64 = (0..last_start_month)
                .map(|start_idx| comp_bal(start_idx, n_months, NONE_REBALANCE_DATA))
                .sum();
            let mean_w_reb = bsum_w_reb / last_start_month as f64;
            let mean_wo_reb = bsum_wo_reb / last_start_month as f64;
            RebalanceStatRecord {
                mean_w_reb,
                mean_wo_reb,
                n_months,
            }
        })
        .collect();
    Ok(RebalanceStats { records })
}

pub struct BestRebalanceTrigger {
    pub best: (RebalanceTrigger, f64),
    pub with_best_dev: (RebalanceTrigger, f64),
    pub with_best_interval: (RebalanceTrigger, f64),
}

pub fn best_rebalance_trigger(
    price_devs: &[&[f64]],
    initial_balances: &[f64],
    monthly_payments: Option<&[&[f64]]>,
) -> BlcResult<BestRebalanceTrigger> {
    let shortest_len = find_shortestlen(price_devs)?;
    let months_to_test = 0..(shortest_len / 2);
    let deviations_to_test = (0..10).chain((20..50).step_by(10)).chain(iter::once(75));
    let triggers: Vec<(RebalanceTrigger, f64)> = months_to_test
        .flat_map(move |n_months| {
            iter::repeat(n_months)
                .zip(deviations_to_test.clone())
                .map(move |(n_months, d)| {
                    let fractions = balances_to_fractions(initial_balances);
                    let rebalance_data = if n_months == 0 && d == 0 {
                        NONE_REBALANCE_DATA
                    } else {
                        let trigger = if n_months == 0 {
                            RebalanceTrigger::from_dev(d as f64 / 100.0)
                        } else if d == 0 {
                            RebalanceTrigger::from_interval(n_months)
                        } else {
                            RebalanceTrigger::from_both(n_months, d as f64 / 100.0)
                        };
                        RebalanceData {
                            trigger,
                            fractions: &fractions,
                        }
                    };
                    let trigger = rebalance_data.trigger;
                    let (balance, _) = compute_total_balance(
                        price_devs,
                        initial_balances,
                        monthly_payments,
                        rebalance_data,
                    );
                    (trigger, balance)
                })
        })
        .collect();
    let (best_trigger, best_balance) = triggers
        .iter()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
        .ok_or(blcerr!("could not find best trigger"))?;
    let (best_dev, best_dev_balance) = triggers
        .iter()
        .filter(|(t, _)| t.interval.is_none())
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
        .ok_or(blcerr!("could not find best trigger"))?;
    let (best_interval, best_interval_balance) = triggers
        .iter()
        .filter(|(t, _)| t.deviation.is_none())
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
        .ok_or(blcerr!("could not find best trigger"))?;

    Ok(BestRebalanceTrigger {
        best: (*best_trigger, *best_balance),
        with_best_dev: (*best_dev, *best_dev_balance),
        with_best_interval: (*best_interval, *best_interval_balance),
    })
}

fn compute_total_balance(
    price_devs: &[&[f64]],
    initial_balances: &[f64],
    monthly_payments: Option<&[&[f64]]>,
    rebalance_data: RebalanceData<'_>,
) -> (f64, f64) {
    if let Ok(total_balance_over_months) = compute_balance_over_months(
        price_devs,
        initial_balances,
        monthly_payments,
        rebalance_data,
    ) {
        total_balance_over_months.last().unwrap()
    } else {
        (initial_balances.iter().sum(), initial_balances.iter().sum())
    }
}
#[test]
fn test_adapt() {
    let price_dev = vec![3.0, 6.0, 12.0, 6.0];
    let price_ref = vec![10.0, 20.0, 40.0, 20.0];
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

    let (b, p) = compute_total_balance(
        &[&world_vals, &em_vals],
        &[0.5, 0.5],
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: Some(rebalance_interval),
                deviation: None,
            },
            fractions: &[0.5, 0.5],
        },
    );
    assert!((b - 2.25).abs() < 1e-12);
    assert!((p - 1.0).abs() < 1e-12);

    let (b, p) = compute_total_balance(
        &[&world_vals, &em_vals],
        &[7.0, 3.0],
        None,
        NONE_REBALANCE_DATA,
    );
    assert!((b - 31.0).abs() < 1e-12);
    assert!((p - 10.0).abs() < 1e-12);

    let (x, p) = compute_total_balance(
        &[&world_vals, &em_vals],
        &[0.7, 0.3],
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: Some(rebalance_interval),
                deviation: None,
            },
            fractions: &[0.7, 0.3],
        },
    );
    assert!((x - 2.89).abs() < 1e-12);
    assert!((p - 1.0).abs() < 1e-12);

    let (x, p) = compute_total_balance(
        &[&world_vals, &em_vals],
        &[1.0, 0.0],
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: Some(rebalance_interval),
                deviation: None,
            },
            fractions: &[1.0, 0.0],
        },
    );
    assert!((x - 4.0).abs() < 1e-12);
    assert!((p - 1.0).abs() < 1e-12);

    let world_vals = vec![1.0; 24];
    let em_vals = vec![1.0; 24];
    let (x, p) = compute_total_balance(
        &[&world_vals, &em_vals],
        &[0.7, 0.3],
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: Some(rebalance_interval),
                deviation: None,
            },
            fractions: &[0.7, 0.3],
        },
    );
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
        &[0.7, 0.3],
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: Some(11),
                deviation: None,
            },
            fractions: &[0.7, 0.3],
        },
    );
    assert!((x - 1.1).abs() < 1e-12);
    assert!((p - 1.0).abs() < 1e-12);
}

#[test]
fn test_compound() {
    let compound_interest: Vec<f64> = random_walk(5.0, true, 0.0, 240).unwrap();
    let (b, p) =
        compute_total_balance(&[&compound_interest], &[10000.0], None, NONE_REBALANCE_DATA);
    assert!((b - 26532.98).abs() < 1e-2);
    assert!((p - 10000.0).abs() < 1e-12);

    let compound_interest: Vec<f64> = random_walk(5.0, true, 0.0, 360).unwrap();
    let ci_len = compound_interest.len();
    let monthly_payments: Vec<f64> = vec![1000.0; ci_len - 1];
    let (b, _) = compute_total_balance(
        &[&compound_interest],
        &[10000.0],
        Some(&[&monthly_payments]),
        NONE_REBALANCE_DATA,
    );
    println!("{b}");
    assert!((b - 861917.27).abs() < 1e-2);
}

#[test]
fn test_rebalance() {
    let v1s = vec![1.0, 1.0, 0.0];
    let v2s = vec![1.0, 1.0, 1.0];
    let (x, _): (Vec<_>, Vec<_>) = compute_balance_over_months(
        &[&v1s, &v2s],
        &[0.5, 0.5],
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: Some(1),
                deviation: None,
            },
            fractions: &[0.5, 0.5],
        },
    )
    .unwrap()
    .unzip();
    assert!((x[2] - 0.5).abs() < 1e-12);

    let v1s = vec![1.0, 1.0, 1.0];
    let v2s = vec![1.0, 0.5, 1.0];
    let (x, _): (Vec<_>, Vec<_>) = compute_balance_over_months(
        &[&v1s, &v2s],
        &[0.5, 0.5],
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: None,
                deviation: Some(0.1),
            },
            fractions: &[0.5, 0.5],
        },
    )
    .unwrap()
    .unzip();
    assert!((x[2] - 1.125).abs() < 1e-12);
}

#[test]
fn test_besttrigger() {
    let v1s = vec![1.0, 1.0, 1.0, 1.0, 0.5, 1.0];
    let v2s = vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    let (_, balance) = best_rebalance_trigger(&[&v1s, &v2s], &[0.5, 0.5], None)
        .unwrap()
        .best;
    assert!((balance - 1.125).abs() < 1e-12);
}
#[test]
fn test_rebalancestats() {
    let v1s = vec![1.0, 1.0, 1.0, 1.0, 0.5, 1.0];
    let v2s = vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    let min_n_months = 3;
    let stats = rebalance_stats(
        &[&v1s, &v2s],
        &[0.5, 0.5],
        None,
        RebalanceData {
            trigger: RebalanceTrigger {
                interval: Some(1),
                deviation: None,
            },
            fractions: &[0.5, 0.5],
        },
        min_n_months,
    )
    .unwrap();
    assert!(stats.records.len() == min_n_months + 1);
    let ref_means_wo = vec![0.9375, 0.9166666666666666, 0.875, 1.0];
    let ref_means_w = vec![0.96875, 0.9583333333333334, 0.9375, 1.125];
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
