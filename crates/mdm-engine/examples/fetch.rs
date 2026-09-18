//! Try the engine for real: `cargo run --example fetch -- <url> [dir] [connections]`

use std::path::PathBuf;

use mdm_engine::{DownloadSpec, Engine, EngineConfig, Outcome, RequestExtras};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let Some(url) = args.next() else {
        eprintln!("usage: fetch <url> [dir] [connections]");
        std::process::exit(2);
    };
    let dir = PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
    let conns: u8 = args.next().and_then(|c| c.parse().ok()).unwrap_or(8);

    let engine = Engine::new(EngineConfig {
        max_connections: conns,
        ..Default::default()
    })
    .unwrap();
    let handle = match engine
        .start(DownloadSpec {
            url: url.parse().expect("a valid http(s) URL"),
            dir,
            filename: None,
            extras: RequestExtras::default(),
            resume_from: None,
        })
        .await
    {
        Ok(h) => h,
        Err(e) => {
            eprintln!("{} ({})", e, e.code());
            std::process::exit(1);
        }
    };
    let p = handle.probe();
    println!("{}  size={:?}  ranges={}", p.filename, p.size, p.ranges);

    let mut rx = handle.subscribe();
    let printer = tokio::spawn(async move {
        while rx.changed().await.is_ok() {
            let pr = rx.borrow().clone();
            let pct = pr.total.map(|t| {
                if t == 0 {
                    100.0
                } else {
                    pr.downloaded as f64 * 100.0 / t as f64
                }
            });
            let bars: String = pr
                .segments
                .iter()
                .map(|s| {
                    let len = s.end - s.start + 1;
                    match s.downloaded * 8 / len.max(1) {
                        8.. => '█',
                        n => "▁▂▃▄▅▆▇".chars().nth((n as usize).min(6)).unwrap_or('▁'),
                    }
                })
                .collect();
            print!(
                "\r{:>6.2}%  {:>8.1} KiB/s  eta {:>4}s  {} {:?}      ",
                pct.unwrap_or(0.0),
                pr.speed_bps as f64 / 1024.0,
                pr.eta_secs.map_or("?".into(), |e| e.to_string()),
                bars,
                pr.status
            );
        }
    });
    let out = handle.wait().await;
    let _ = printer.await;
    println!();
    match out {
        Outcome::Completed(path) => println!("saved {}", path.display()),
        Outcome::Failed { error, .. } => {
            eprintln!("failed: {} ({})", error, error.code());
            std::process::exit(1);
        }
        other => println!("{other:?}"),
    }
}
