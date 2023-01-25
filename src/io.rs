use std::error::Error;

pub fn read_csv_from_str(csv: &str) -> Result<(Vec<usize>, Vec<f64>), Box<dyn Error>> {
    let reader = csv::Reader::from_reader(csv.as_bytes());
    read_csv(reader)
}

fn read_csv<R>(mut reader: csv::Reader<R>) -> Result<(Vec<usize>, Vec<f64>), Box<dyn Error>>
where
    R: std::io::Read,
{
    let (dates, values): (Vec<usize>, Vec<f64>) = reader
        .records()
        .flat_map(|record| -> Result<Option<(usize, f64)>, Box<dyn Error>> {
            let record = record?;
            if let (Some(date), Some(val)) = (record.get(0), record.get(1)) {
                let year: usize = date[..4].parse()?;
                let month: usize = date[5..].parse()?;
                let val: f64 = val.parse()?;
                let date = year * 100 + month;
                Ok(Some((date, val)))
            } else {
                Ok(None)
            }
        })
        .flatten()
        .unzip();

    // validate all months are there
    for (d1, d2) in dates.iter().zip(dates[1..].iter()) {
        if d1 - (d1 / 100) * 100 == 12 {
            assert_eq!(d2 - d1, 89);
        } else {
            assert_eq!(d2 - d1, 1);
        }
    }
    Ok((dates, values))
}
