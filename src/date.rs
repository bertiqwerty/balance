use std::iter;
use std::{fmt::Display, ops::Add, ops::Sub, str::FromStr};

use crate::core_types::BlcError;
use crate::{
    blcerr,
    core_types::{to_blc, BlcResult},
};

fn n_month_between_dates(earlier: Date, later: Date) -> Option<usize> {
    if earlier > later {
        None
    } else {
        let year_diff = later.year() - earlier.year();
        let (e_month, l_month) = (earlier.month(), later.month());
        let (months_diff, year_correction) = if l_month >= e_month {
            (l_month - e_month, 0)
        } else {
            (12 - e_month + l_month, 1)
        };
        Some(12 * (year_diff - year_correction) + months_diff)
    }
}

pub fn date_after_nmonths(t0: Date, n_months: usize) -> Date {
    let n_total_months = n_months + t0.year() * 12 + t0.month();
    let new_year = n_total_months / 12;
    let new_month = n_total_months % 12;
    let (new_year, new_month) = if new_month == 0 {
        (new_year - 1, 12)
    } else {
        (new_year, new_month)
    };
    Date::new(new_year, new_month).unwrap()
}

pub fn fill_between(start: Date, end: Date) -> Vec<Date> {
    iter::successors(Some(start), |d| {
        if d < &end {
            Some(d.next_month())
        } else {
            None
        }
    })
    .collect()
}

#[derive(Clone, Copy, Debug)]
pub struct IntervalIter {
    end: Date,
    current: Date,
    len_in_months: usize,
}
impl Iterator for IntervalIter {
    type Item = Date;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current > self.end {
            None
        } else {
            let res = Some(self.current);
            self.current = self.current.next_month();
            res
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len_in_months, Some(self.len_in_months))
    }
}

/// Intervals include both, start and end
#[derive(Clone, Copy, Debug)]
pub struct Interval {
    start: Date,
    end: Date,
    len_in_months: usize,
}
impl Interval {
    pub fn new(start: Date, end: Date) -> BlcResult<Self> {
        Ok(Self {
            start,
            end,
            len_in_months: start.n_month_until(end)? + 1,
        })
    }
    pub fn len(&self) -> usize {
        self.len_in_months
    }
    pub fn start(&self) -> Date {
        self.start
    }
    pub fn end(&self) -> Date {
        self.end
    }
    pub fn contains(&self, d: Date) -> bool {
        self.start <= d && d <= self.end
    }
}
impl IntoIterator for &Interval {
    type IntoIter = IntervalIter;
    type Item = Date;
    fn into_iter(self) -> Self::IntoIter {
        IntervalIter {
            current: self.start,
            end: self.end,
            len_in_months: self.len_in_months,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    date: usize,
}
impl Date {
    pub fn new(year: usize, month: usize) -> BlcResult<Self> {
        if month == 0 || month > 12 {
            Err(blcerr!("we only have months from 1-12 but not {month}"))
        } else if year == 0 {
            Err(blcerr!("there was no year 0"))
        } else {
            Ok(Date {
                date: year * 100 + month,
            })
        }
    }

    pub fn year(&self) -> usize {
        self.date / 100
    }

    pub fn month(&self) -> usize {
        self.date % 100
    }

    pub fn next_month(&self) -> Date {
        if self.month() == 12 {
            Date::new(self.year() + 1, 1).unwrap()
        } else {
            Date::new(self.year(), self.month() + 1).unwrap()
        }
    }

    pub fn n_month_until(&self, later: Date) -> BlcResult<usize> {
        (later - *self).ok_or_else(|| blcerr!("later must be after self"))
    }
}
impl Add<usize> for Date {
    type Output = BlcResult<Date>;
    fn add(self, rhs: usize) -> Self::Output {
        let month = self.month() + rhs;
        let year = self.year() + month / 12;
        let month = month % 12;
        let month = if month == 0 { 12 } else { month };
        Date::new(year, month)
    }
}
impl Sub for Date {
    type Output = Option<usize>;
    fn sub(self, rhs: Self) -> Self::Output {
        n_month_between_dates(rhs, self)
    }
}
impl Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let year = self.year();
        let month = self.month();
        let s = format!("{year:04}/{month:02}");
        f.write_str(&s)
    }
}
impl FromStr for Date {
    type Err = BlcError;
    fn from_str(d: &str) -> Result<Self, Self::Err> {
        if d.len() == 7 {
            let year = d[..4].parse::<usize>().map_err(to_blc)?;
            let month = d[5..].parse::<usize>().map_err(to_blc)?;
            Self::new(year, month)
        } else {
            Err(blcerr!("date needs 7 digits, YYYY/MM, got {d}"))
        }
    }
}

#[test]
fn test_fromym() {
    fn test(year: usize, month: usize, reference: usize) {
        assert_eq!(Date::new(year, month).unwrap(), Date { date: reference });
    }
    test(2000, 1, 200001);
    test(1999, 12, 199912);
    test(1, 8, 108);
    test(2023, 5, 202305);

    assert!(Date::new(0, 7).is_err());
    assert!(Date::new(0, 31).is_err());
    assert!(Date::new(2022, 31).is_err());
    assert!(Date::new(1990, 13).is_err());
    assert!(Date::new(0, 0).is_err());
    assert!(Date::new(2017, 0).is_err());
}

#[test]
fn test_dateaftermonth() {
    fn test(year: usize, month: usize, n_months: usize, reference: usize) {
        assert_eq!(
            date_after_nmonths(Date::new(year, month).unwrap(), n_months),
            Date { date: reference }
        );
    }
    test(1990, 1, 12, 199101);
    test(1990, 1, 11, 199012);
    test(2023, 12, 13, 202501);
    test(2023, 7, 13, 202408);
    test(2023, 11, 1, 202312);
    test(2023, 11, 3, 202402);
}

#[test]
fn test_year_month() {
    fn test(d: &str, reference: usize, year: usize, month: usize) {
        let d = Date::from_str(d).unwrap();
        assert_eq!(d, Date { date: reference });
        assert_eq!(d.year(), year);
        assert_eq!(d.month(), month);
    }
    test("1987/12", 198712, 1987, 12);
    test("1988/01", 198801, 1988, 1);
    test("2011/10", 201110, 2011, 10);
    test("1997/11", 199711, 1997, 11);
    assert!(Date::from_str("d").is_err());
    assert!(Date::from_str("199912").is_err());
    assert!(Date::from_str("1999/00").is_err());
    assert!(Date::from_str("1999/1").is_err());
}

#[test]
fn test_nextmonth() {
    fn test(year: usize, month: usize, reference: usize) {
        assert_eq!(
            Date::new(year, month).unwrap().next_month(),
            Date { date: reference }
        );
    }
    test(2022, 12, 202301);
    test(2022, 1, 202202);
}

#[test]
fn test_tostring() {
    assert_eq!(&Date::from_str("1988/12").unwrap().to_string(), "1988/12")
}

#[test]
fn test_arith() {
    let d1 = Date::from_str("1988/12").unwrap();
    let d2 = Date::from_str("1989/01").unwrap();
    assert_eq!((d1 + 1).unwrap(), d2);
    assert_eq!(d2 - d1, Some(1));
    assert_eq!(d1 - d2, None);
    let d1 = Date::from_str("1988/02").unwrap();
    let d2 = Date::from_str("1999/01").unwrap();
    assert_eq!(((d1 + 10 * 12).unwrap() + 11).unwrap(), d2);
}

#[test]
fn test_interval() {
    let d1 = Date::from_str("1988/02").unwrap();
    let d2 = Date::from_str("1999/01").unwrap();
    let inter = Interval::new(d1, d2).unwrap();
    assert_eq!(inter.len(), 132);
    assert!(inter.contains(d1));
    assert!(inter.contains(d2));
    assert!(inter.contains(Date::from_str("1989/07").unwrap()));
}
