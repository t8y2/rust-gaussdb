use std::io::{self, BufRead, Write};

fn main() {
    // Signal readiness to the DBX main process
    println!("{{\"ready\":true}}");
    io::stdout().flush().unwrap();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap();

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => {
                let trimmed = l.trim().to_string();
                if trimmed.is_empty() {
                    continue;
                }
                trimmed
            }
            Err(_) => break,
        };
        let response = rt.block_on(gaussdb_agent::protocol::handle_request(&line));
        println!("{}", response);
        io::stdout().flush().unwrap();
    }
}
