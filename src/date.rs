use std::fmt::Display;

use crate::{
    blcerr,
    core_types::{to_blc, BlcResult},
};

pub fn n_month_between_dates(earlier: Date, later: Date) -> BlcResult<usize> {
    if earlier > later {
        Err(blcerr!("{earlier} is after {later}"))
    } else {
        let year_diff = later.year() - earlier.year();
        let (e_month, l_month) = (earlier.month(), later.month());
        let (months_diff, year_correction) = if l_month >= e_month {
            (l_month - e_month, 0)
        } else {
            (12 - e_month + l_month, 1)
        };
        Ok(12 * (year_diff - year_correction) + months_diff)
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

    pub fn from_str(d: &str) -> BlcResult<Self> {
        if d.len() == 7 {
            let year = d[..4].parse::<usize>().map_err(to_blc)?;
            let month = d[5..].parse::<usize>().map_err(to_blc)?;
            Self::new(year, month)
        } else {
            Err(blcerr!("date needs 7 digits, YYYY/MM, got {d}"))
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
}
impl Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let year = self.year();
        let month = self.month();
        let s = format!("{year:04}/{month:02}");
        f.write_str(&s)
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
