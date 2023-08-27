use crate::{
    core_types::{to_blc, BlcResult},
    date::Date,
};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
pub const URL_WRITE_SHARELINK: &str = "https://bertiqwerty.com/balance_storage/write.php";
pub const URL_READ_SHARELINK: &str = "https://bertiqwerty.com/balance_storage/read.php";

pub fn sessionid_to_link(session_id: &str) -> String {
    format!("https://bertiqwerty.com/index.html?session_id={session_id}")
}

pub fn sessionid_from_link(link: &str) -> Option<String> {
    link.split('?')
        .last()
        .and_then(|s| s.split("session_id=").last())
        .map(|s| s.chars().take_while(|c| c.is_alphanumeric()).collect::<String>())
}

#[derive(Serialize, Deserialize)]
pub struct ResponsePayload<T> {
    pub status: u16,
    pub message: String,
    pub json_data: T,
}

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
