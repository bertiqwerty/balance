use rand::thread_rng;
use rand_distr::{Distribution, Normal};

pub struct RebalanceData<'a> {
    interval: usize,
    fractions: &'a [f64],
}

pub fn compute_balance<'a>(
    price_devs: &[&[f64]],
    initial_balances: &[f64],
    rebalance_interval: Option<RebalanceData<'a>>,
) -> f64 {
    let mut balances: Vec<f64> = initial_balances.iter().copied().collect();
    for (idx_prev, idx) in (0..price_devs[0].len()).zip(1..price_devs[0].len()) {
        price_devs
            .iter()
            .zip(balances.iter_mut())
            .for_each(|(pd, balance)| *balance = *balance * pd[idx] / pd[idx_prev]);
        let total: f64 = balances.iter().sum();
        match &rebalance_interval {
            Some(rbd) if idx % rbd.interval == 0 => {
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

const N_SIGMAS: usize = 12;


pub fn random_walk(mu: f64, sigma_mean: f64, n_months: usize) -> Vec<f64> {
    let sigma_distribution = Normal::new(sigma_mean, sigma_mean).unwrap();
    let mut sigma_rng = thread_rng();
    let mut rv_rng = thread_rng();
    let mut res = vec![1.0; n_months];
    let mut last_sigmas = [sigma_mean; N_SIGMAS];
    for (i, sigma) in (1..n_months).zip(sigma_distribution.sample_iter(&mut sigma_rng)) {
        for i in 0..9 {
            let tmp = last_sigmas[i + 1];
            last_sigmas[i] = tmp;
        }
        last_sigmas[9] = sigma;
        last_sigmas.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let sigma = last_sigmas[N_SIGMAS / 2].abs();
        let d = Normal::new(mu, sigma).unwrap();
        let rv = d.sample(&mut rv_rng);
        res[i] = res[i - 1] + rv;
    }
    res
}

#[cfg(test)]
use std::{iter, vec};

#[test]
fn test_rebalance() {
    let rebalance_interval = 5;
    let world_vals = iter::repeat(1.0)
        .take(rebalance_interval)
        .chain(iter::repeat(2.0).take(rebalance_interval))
        .chain(iter::repeat(4.0).take(rebalance_interval))
        .collect::<Vec<_>>();
    let em_vals = vec![1.0; rebalance_interval * 3];
    let x = compute_balance(
        &[&world_vals, &em_vals],
        &[0.5, 0.5],
        Some(RebalanceData {
            interval: rebalance_interval,
            fractions: &[0.5, 0.5],
        }),
    );
    assert!((x - 2.25).abs() < 1e-12);
    let x = compute_balance(&[&world_vals, &em_vals], &[7.0, 3.0], None);
    assert!((x - 31.0).abs() < 1e-12);
    let x = compute_balance(
        &[&world_vals, &em_vals],
        &[0.7, 0.3],
        Some(RebalanceData {
            interval: rebalance_interval,
            fractions: &[0.7, 0.3],
        }),
    );
    assert!((x - 2.89).abs() < 1e-12);
    let x = compute_balance(
        &[&world_vals, &em_vals],
        &[1.0, 0.0],
        Some(RebalanceData {
            interval: rebalance_interval,
            fractions: &[1.0, 0.0],
        }),
    );
    assert!((x - 4.0).abs() < 1e-12);
    let world_vals = vec![1.0; 24];
    let em_vals = vec![1.0; 24];
    let x = compute_balance(
        &[&world_vals, &em_vals],
        &[0.7, 0.3],
        Some(RebalanceData {
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
    let x = compute_balance(
        &[&world_vals, &em_vals],
        &[0.7, 0.3],
        Some(RebalanceData {
            interval: 11,
            fractions: &[0.7, 0.3],
        }),
    );
    assert!((x - 1.1).abs() < 1e-12);
}
