use {
    anyhow::Result,
    ini::ini,
    rodbus::prelude::*,
    std::{collections::HashMap, time::Duration},
};

#[tokio::main(basic_scheduler)]
async fn main() -> Result<()> {
    setup_logging().expect("failed to initialize logging");

    if let Err(ref e) = run().await {
        println!("error: {}", e);
    }

    Ok(())
}

fn setup_logging() -> Result<()> {
    let base_config = fern::Dispatch::new().format(|out, message, record| {
        out.finish(format_args!(
            "{}[{}][{}] {}",
            chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
            record.target(),
            record.level(),
            message
        ))
    });

    let stdout_config = fern::Dispatch::new()
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout());

    let file_config = fern::Dispatch::new().level(log::LevelFilter::Trace).chain(
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true) // start log file anew each run
            .open("debug.log")?,
    );

    base_config
        .chain(stdout_config)
        .chain(file_config)
        .apply()?;

    Ok(())
}

async fn run() -> Result<()> {
    let channel = spawn_tcp_client_task("192.168.1.127:502".parse()?, 10, strategy::default());
    let mut session = channel.create_session(UnitId { value: 1 }, Duration::from_secs(1));

    let registers = ini!("regs.ini");
    let registers = &registers["default"];

    let ranges = build_ranges(&registers)?;

    let mut saved_registers = HashMap::<u16, u16>::new();

    let delay = Duration::from_secs_f64(0.5 + 0.1 * registers.len() as f64);

    loop {
        run_command(&mut session, &registers, &mut saved_registers, &ranges).await?;
        tokio::time::delay_for(delay).await
    }
}

/// Convert a disjoint set of modbus addresses into address ranges
fn build_ranges(registers: &HashMap<String, Option<String>>) -> Result<Vec<AddressRange>> {
    let mut output: Vec<AddressRange> = vec![];
    for (key, _) in registers {
        let key = key.parse::<u16>()?;
        // Pessimistic O^2 algorithm
        let mut added = false;
        for mut x in &mut output {
            if key == x.start - 1 {
                x.start = key;
                x.count += 1;
                added = true;
            } else if key == x.start + x.count {
                x.count += 1;
                added = true;
            } else {
                // add new entry
            }
        }
        if !added {
            output.push(AddressRange {
                start: key,
                count: 1,
            });
        }
    }
    Ok(output)
}

async fn run_command(
    session: &mut AsyncSession,
    registers: &HashMap<String, Option<String>>,
    saved_registers: &mut HashMap<u16, u16>,
    ranges: &Vec<AddressRange>,
) -> Result<()> {
    for range in ranges {
        match session.read_holding_registers(*range).await {
            Ok(points) => {
                for x in points {
                    let desc = registers[&format!("{}", x.index)].clone().unwrap();
                    let old_value = saved_registers.entry(x.index).or_insert(0);
                    if *old_value != x.value {
                        println!("{}/{}: {} -> {}", x.index, desc, *old_value, x.value);
                    }
                    *old_value = x.value;
                }
            }
            Err(e) => println!("Error, ignored: {}", e),
        }
    }
    Ok(())
}
