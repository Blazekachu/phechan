use phechan_sat::{simulate_sat_flow, SatLocation};

use crate::args::flag_value;

pub fn dispatch(args: &[String]) -> Result<(), String> {
    match args.get(1).map(String::as_str) {
        Some("select") => select(&args[2..]),
        Some(other) => Err(format!("unknown sat subcommand '{other}' (select)")),
        None => Err("usage: phechan sat select --inputs 1000,5000 --outputs 1000,4800 --at 0:0".into()),
    }
}

/// Report where a sat lands. `--at <input_index>:<offset>`
fn select(args: &[String]) -> Result<(), String> {
    let inputs = parse_list(&flag_value(args, "--inputs").ok_or("--inputs required")?)?;
    let outputs = parse_list(&flag_value(args, "--outputs").ok_or("--outputs required")?)?;
    let at = flag_value(args, "--at").ok_or("--at <vin>:<offset> required")?;
    let (vin_s, off_s) = at.split_once(':').ok_or("bad --at")?;
    let vin: usize = vin_s.parse().map_err(|_| "bad vin")?;
    let offset: u64 = off_s.parse().map_err(|_| "bad offset")?;
    let flow = simulate_sat_flow(&inputs, &outputs).map_err(|e| e.to_string())?;
    let loc = flow
        .location_of_input_sat(vin, offset)
        .ok_or("sat out of range")?;
    println!("fee: {}", flow.fee);
    match loc {
        SatLocation::Output { vout } => println!("location: output:{vout}"),
        SatLocation::Fee => println!("location: fee"),
    }
    Ok(())
}

fn parse_list(s: &str) -> Result<Vec<u64>, String> {
    s.split(',')
        .map(|p| p.trim().parse::<u64>().map_err(|_| format!("bad number '{p}'")))
        .collect()
}
