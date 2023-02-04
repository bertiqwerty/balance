use crate::{
    blcerr,
    core_types::{to_blc, BlcResult},
};
use rand::{rngs::StdRng, SeedableRng};
use rand_distr::{Distribution, Normal};
use std::iter;

#[derive(Clone, Debug)]
pub struct RebalanceData<'a> {
    /// after how many months is re-balancing applied
    pub interval: usize,
    /// fractions of the indices
    pub fractions: &'a [f64],
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
    rebalance_data: Option<RebalanceData<'a>>,
) -> BlcResult<impl Iterator<Item = (f64, f64)> + 'a> {
    let total_initial_balances: f64 = initial_balances.iter().sum();
    let shortest_len = price_devs
        .iter()
        .map(|pd| pd.len())
        .min()
        .ok_or(blcerr!("empty price dev"))?;
    let balances_over_months = (0..shortest_len).zip(1..shortest_len).scan(
        (initial_balances.to_vec(), 0.0),
        move |(balances, monthly_payments_upto_now), (i_prev_month, i_month)| {
            // update the balance for each security at the current month
            for i_sec in 0..balances.len() {
                let payment_this_month = monthly_payments
                    .map(|mp| mp[i_sec][i_month - 1])
                    .unwrap_or(0.0);
                let price_update =
                    balances[i_sec] * price_devs[i_sec][i_month] / price_devs[i_sec][i_prev_month];
                balances[i_sec] = payment_this_month + price_update;
                *monthly_payments_upto_now += payment_this_month;
            }

            let total: f64 = balances.iter().sum();
            match &rebalance_data {
                Some(rbd) if rbd.interval > 0 && i_month % rbd.interval == 0 => {
                    rbd.fractions
                        .iter()
                        .zip(balances.iter_mut())
                        .for_each(|(frac, balance)| {
                            *balance = frac * total;
                        });
                }
                _ => (),
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
pub fn _adapt_pricedev_to_initial_balance<'a>(
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
fn unix_to_now_nanos() -> BlcResult<u64> {
    use wasm_bindgen::prelude::*;
    let now = (js_sys::Date::now() * 1000.0) as u128;
    Ok((now % (u64::MAX as u128)) as u64)
}

#[cfg(not(target_arch = "wasm32"))]
fn unix_to_now_nanos() -> BlcResult<u64> {
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
    sigma_mean: f64,
    n_months: usize,
) -> BlcResult<Vec<f64>> {
    let mut sigma_rng = StdRng::seed_from_u64(unix_to_now_nanos()?);
    let sigma_distribution = Normal::new(sigma_mean, sigma_mean).map_err(to_blc)?;
    let mut last_sigmas = [sigma_mean; SIGMA_WINDOW_SIZE];
    let mut rv_rng = StdRng::seed_from_u64(unix_to_now_nanos()?);
    let mut price = 1.0;
    let mut res = vec![price; n_months + 1];

    let mu_price_pair = |price, i_month| {
        if (i_month) % 12 == 0 {
            (
                price * expected_yearly_return / 1200.0,
                price * (1.0 + expected_yearly_return / 100.0),
            )
        } else {
            (price * expected_yearly_return / 1200.0, price)
        }
    };
    for (i, sigma) in (1..(n_months + 1)).zip(sigma_distribution.sample_iter(&mut sigma_rng)) {
        for i in 0..9 {
            last_sigmas[i] = last_sigmas[i + 1];
        }
        last_sigmas[9] = sigma;
        last_sigmas.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let sigma = last_sigmas[SIGMA_WINDOW_SIZE / 2].abs();
        let mpp = mu_price_pair(price, i);
        let mu = mpp.0;
        price = mpp.1;
        let d = Normal::new(mu, sigma).map_err(to_blc)?;
        let rv = d.sample(&mut rv_rng);
        res[i] = res[i - 1] + rv;
    }
    Ok(res)
}

#[cfg(test)]
use std::vec;

#[cfg(test)]
fn compute_total_balance(
    price_devs: &[&[f64]],
    initial_balances: &[f64],
    monthly_payments: Option<&[&[f64]]>,
    rebalance_data: Option<RebalanceData<'_>>,
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
    let adapted = _adapt_pricedev_to_initial_balance(10.0, &price_dev);
    for (a, p) in adapted.zip(price_ref.iter()) {
        assert!((a - p) < 1e-12);
    }
}

#[test]
fn test_rebalance() {
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
        Some(RebalanceData {
            interval: rebalance_interval,
            fractions: &[0.5, 0.5],
        }),
    );
    assert!((b - 2.25).abs() < 1e-12);
    assert!((p - 1.0).abs() < 1e-12);

    let (b, p) = compute_total_balance(&[&world_vals, &em_vals], &[7.0, 3.0], None, None);
    assert!((b - 31.0).abs() < 1e-12);
    assert!((p - 10.0).abs() < 1e-12);

    let (x, p) = compute_total_balance(
        &[&world_vals, &em_vals],
        &[0.7, 0.3],
        None,
        Some(RebalanceData {
            interval: rebalance_interval,
            fractions: &[0.7, 0.3],
        }),
    );
    assert!((x - 2.89).abs() < 1e-12);
    assert!((p - 1.0).abs() < 1e-12);

    let (x, p) = compute_total_balance(
        &[&world_vals, &em_vals],
        &[1.0, 0.0],
        None,
        Some(RebalanceData {
            interval: rebalance_interval,
            fractions: &[1.0, 0.0],
        }),
    );
    assert!((x - 4.0).abs() < 1e-12);
    assert!((p - 1.0).abs() < 1e-12);

    let world_vals = vec![1.0; 24];
    let em_vals = vec![1.0; 24];
    let (x, p) = compute_total_balance(
        &[&world_vals, &em_vals],
        &[0.7, 0.3],
        None,
        Some(RebalanceData {
            interval: 12,
            fractions: &[0.7, 0.3],
        }),
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
        Some(RebalanceData {
            interval: 11,
            fractions: &[0.7, 0.3],
        }),
    );
    assert!((x - 1.1).abs() < 1e-12);
    assert!((p - 1.0).abs() < 1e-12);

    let compound_interest: Vec<f64> = random_walk(5.0, 0.0, 240).unwrap();
    let ci_len = compound_interest.len();
    let monthly_payments: Vec<f64> = vec![1000.0; ci_len - 1];
    let (b, p) = compute_total_balance(
        &[&compound_interest],
        &[10000.0],
        Some(&[&monthly_payments]),
        None,
    );
    assert!((b - 432257.37).abs() < 1e-2);
    assert!((p - 250000.0).abs() < 1e-12);
}
