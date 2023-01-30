use crate::core_types::{to_bres, BalResult};
use rand::{rngs::StdRng, SeedableRng};
use rand_distr::{Distribution, Normal};
use std::iter;

pub struct _RebalanceData<'a> {
    /// after how many months is re-balancing applied
    interval: usize,
    /// fractions of the indices
    fractions: &'a [f64],
}

/// Compute the balance given initial values and price developments of securities
///
/// Arguments
/// * `price_devs` - developments of the individual securities (e.g., stock prices, index prices, ...)
///                  2d-vector, first axis addresses the security, second axis is the price
/// * `initial_balances` - initial balance per security (e.g., stock price, index price, ...)
/// * `rebalance_interval` - pass if indices are rebalanced
///
pub fn _compute_balance(
    price_devs: &[&[f64]],
    initial_balances: &[f64],
    rebalance_interval: Option<_RebalanceData<'_>>,
) -> f64 {
    let mut balances: Vec<f64> = initial_balances.to_vec();
    for (idx_prev_month, idx_month) in (0..price_devs[0].len()).zip(1..price_devs[0].len()) {
        // update the balance for each security given at the current month
        price_devs
            .iter()
            .zip(balances.iter_mut())
            .for_each(|(pd, balance)| *balance = *balance * pd[idx_month] / pd[idx_prev_month]);
        let total: f64 = balances.iter().sum();
        match &rebalance_interval {
            Some(rbd) if idx_month % rbd.interval == 0 => {
                rbd.fractions
                    .iter()
                    .zip(balances.iter_mut())
                    .for_each(|(frac, balance)| *balance = frac * total);
            }
            _ => (),
        }
    }
    balances.iter().sum()
}

pub fn _adapt_pricedev_to_initial_balance<'a>(
    mut initial_balance: f64,
    price_dev: &'a [f64],
) -> impl Iterator<Item = f64> + 'a {
    iter::once(initial_balance).chain(
        price_dev[0..price_dev.len()]
            .iter()
            .zip(price_dev[1..].iter())
            .map(move |(pd_prev, pd)| {
                initial_balance = initial_balance * pd / pd_prev;
                initial_balance
            }),
    )
}

#[cfg(target_arch = "wasm32")]
fn unix_to_now_nanos() -> BalResult<u64> {
    use wasm_bindgen::prelude::*;
    let now = (js_sys::Date::now() * 1000.0) as u128;
    Ok((now % (u64::MAX as u128)) as u64)
}

#[cfg(not(target_arch = "wasm32"))]
fn unix_to_now_nanos() -> BalResult<u64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    Ok((SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(to_bres)?
        .as_nanos()
        % (u64::MAX as u128)) as u64)
}

const SIGMA_WINDOW_SIZE: usize = 12;

pub fn random_walk(
    expected_yearly_return: f64,
    sigma_mean: f64,
    n_months: usize,
    initial_balance: f64,
) -> BalResult<Vec<f64>> {
    let mut sigma_rng = StdRng::seed_from_u64(unix_to_now_nanos()?);
    let sigma_distribution = Normal::new(sigma_mean, sigma_mean).map_err(to_bres)?;
    let mut last_sigmas = [sigma_mean; SIGMA_WINDOW_SIZE];
    let mut rv_rng = StdRng::seed_from_u64(unix_to_now_nanos()?);
    let mut price = initial_balance;
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
        let d = Normal::new(mu, sigma).map_err(to_bres)?;
        let rv = d.sample(&mut rv_rng);
        res[i] = res[i - 1] + rv;
    }
    Ok(res)
}

#[cfg(test)]
use std::vec;

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
    let x = _compute_balance(
        &[&world_vals, &em_vals],
        &[0.5, 0.5],
        Some(_RebalanceData {
            interval: rebalance_interval,
            fractions: &[0.5, 0.5],
        }),
    );
    assert!((x - 2.25).abs() < 1e-12);
    let x = _compute_balance(&[&world_vals, &em_vals], &[7.0, 3.0], None);
    assert!((x - 31.0).abs() < 1e-12);
    let x = _compute_balance(
        &[&world_vals, &em_vals],
        &[0.7, 0.3],
        Some(_RebalanceData {
            interval: rebalance_interval,
            fractions: &[0.7, 0.3],
        }),
    );
    assert!((x - 2.89).abs() < 1e-12);
    let x = _compute_balance(
        &[&world_vals, &em_vals],
        &[1.0, 0.0],
        Some(_RebalanceData {
            interval: rebalance_interval,
            fractions: &[1.0, 0.0],
        }),
    );
    assert!((x - 4.0).abs() < 1e-12);
    let world_vals = vec![1.0; 24];
    let em_vals = vec![1.0; 24];
    let x = _compute_balance(
        &[&world_vals, &em_vals],
        &[0.7, 0.3],
        Some(_RebalanceData {
            interval: 12,
            fractions: &[0.7, 0.3],
        }),
    );
    assert!((x - 1.0).abs() < 1e-12);
    let world_vals = iter::repeat(1.0)
        .take(10)
        .chain(iter::once(1.1))
        .collect::<Vec<_>>();
    let em_vals = iter::repeat(1.0)
        .take(10)
        .chain(iter::once(1.1))
        .collect::<Vec<_>>();
    let x = _compute_balance(
        &[&world_vals, &em_vals],
        &[0.7, 0.3],
        Some(_RebalanceData {
            interval: 11,
            fractions: &[0.7, 0.3],
        }),
    );
    assert!((x - 1.1).abs() < 1e-12);
}
