use crate::{
    core_types::{to_blc, BlcResult},
    date::Date,
};
use std::str::FromStr;

pub fn read_csv_from_str(csv: &str) -> BlcResult<(Vec<Date>, Vec<f64>)> {
    let reader = csv::Reader::from_reader(csv.as_bytes());
    read_csv(reader)
}

fn read_csv<R>(mut reader: csv::Reader<R>) -> BlcResult<(Vec<Date>, Vec<f64>)>
where
    R: std::io::Read,
{
    let (dates, values): (Vec<Date>, Vec<f64>) = reader
        .records()
        .flat_map(|record| -> BlcResult<Option<(Date, f64)>> {
            let record = record.map_err(to_blc)?;
            if let (Some(date), Some(val)) = (record.get(0), record.get(1)) {
                let val: f64 = val.parse().map_err(to_blc)?;
                let date = Date::from_str(date)?;
                Ok(Some((date, val)))
            } else {
                Ok(None)
            }
        })
        .flatten()
        .unzip();

    // validate all months are there
    for (d1, d2) in dates.iter().zip(dates[1..].iter()) {
        if d1.month() == 12 {
            assert_eq!(d2.month(), 1);
            assert_eq!(d1.year() + 1, d2.year());
        } else {
            assert_eq!(d2.month() - d1.month(), 1);
            assert_eq!(d2.year() - d1.year(), 0);
        }
    }
    Ok((dates, values))
}
