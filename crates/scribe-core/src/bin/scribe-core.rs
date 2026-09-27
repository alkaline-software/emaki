//! A small CLI over the core: list the index, render one session, sweep the
//! archive, sync search. It exists so the core can be checked without a window
//! and driven from a shell or a scheduled task.

use std::path::PathBuf;

use scribe_core::{adapters, archive, config::Config, redact::Redactor, render_md, search, store};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let cfg = Config::load();
    match cmd {
        "list" => {
            let refs = adapters::index_all(200, &cfg.agents);
            for r in &refs {
                println!(
                    "{:<12} {:<38} {:<9} {:>8} {:<10} {}",
                    r.agent.as_str(),
                    r.session_id,
                    if r.archived { "kept" } else { "live" },
                    r.size,
                    r.state.phase.as_str(),
                    r.title
                );
            }
            eprintln!("{} sessions", refs.len());
        }
        "render" => {
            let target = args.get(1).expect("render <session-id|path>");
            let path = resolve(target, &cfg);
            let (session, _) = (adapters::load_path(&path), ());
            let mut session = session;
            store::annotate(&mut session);
            let redactor = Redactor::from_config(&cfg);
            print!("{}", render_md::render(&session, &cfg, &redactor));
        }
        "json" => {
            let target = args.get(1).expect("json <session-id|path>");
            let path = resolve(target, &cfg);
            let session = adapters::load_path(&path);
            println!("{}", serde_json::to_string_pretty(&session).unwrap());
        }
        "build" => {
            let target = args.get(1).expect("build <session-id|path>");
            let path = resolve(target, &cfg);
            let redactor = Redactor::from_config(&cfg);
            let (session, written) = store::build_one(&path, &cfg, &redactor);
            println!("{} rounds -> {:?}", session.rounds.len(), written);
        }
        "archive" => {
            let refs = adapters::index_all(200, &cfg.agents);
            let stats = archive::sweep(&refs);
            println!("{}", serde_json::to_string(&stats).unwrap());
            println!("{}", serde_json::to_string(&archive::summary()).unwrap());
        }
        "sync" => {
            let refs = adapters::index_all(200, &cfg.agents);
            let mut idx = search::SearchIndex::open().expect("open search index");
            let report = idx.sync(&refs, args.iter().any(|a| a == "--force"));
            println!("{}", serde_json::to_string(&report).unwrap());
        }
        "search" => {
            let q = args[1..].join(" ");
            let idx = search::SearchIndex::open().expect("open search index");
            let res = idx.search(&q, 300, 4);
            for s in &res.sessions {
                println!("{} {} [{}] {} hits: {}", s.agent, s.id, s.project, s.hits, s.title);
                for m in &s.matches {
                    println!("    r{} {} {}", m.round, m.kind, m.snippet.replace('\x02', "[").replace('\x03', "]").replace('\n', " "));
                }
            }
            eprintln!("{} hits in {} sessions", res.total, res.sessions.len());
        }
        "drive" => {
            // scribe-core drive <cwd> <message...>: start a fresh headless
            // session, send one message, print events until the turn ends.
            let cwd = args.get(1).expect("drive <cwd> <message>").clone();
            let text = args[2..].join(" ");
            let id = format!("{}", uuid_v4());
            let (tx, rx) = std::sync::mpsc::channel();
            let started = std::time::Instant::now();
            let d = scribe_core::driver::Driver::start(&id, &cwd, false, "", "", tx).expect("start claude");
            println!("started {id} in {:.1}s; mode={} model={}", started.elapsed().as_secs_f64(), d.mode(), d.model());
            let caps = d.caps();
            println!("commands: {} slash: {} skills: {} tools: {}", caps.commands.len(), caps.slash_commands.len(), caps.skills.len(), caps.tools.len());
            d.send(&text, Vec::new()).expect("send");
            loop {
                match rx.recv_timeout(std::time::Duration::from_secs(180)) {
                    Ok(scribe_core::driver::Event::Permission(p)) => {
                        println!("permission: {} {}", p.tool_name, serde_json::to_string(&p.input).unwrap());
                        d.answer_permission(&p.request_id, true, "");
                    }
                    Ok(scribe_core::driver::Event::Result(r)) => {
                        println!("result: {} error={} {}ms ${:.4}", r.subtype, r.is_error, r.duration_ms, r.cost_usd);
                        break;
                    }
                    Ok(scribe_core::driver::Event::Exit { code, error }) => {
                        println!("exit: {code:?} {error}");
                        break;
                    }
                    Ok(other) => println!("event: {other:?}"),
                    Err(_) => {
                        println!("timeout");
                        break;
                    }
                }
            }
            d.stop();
            let path = scribe_core::paths::projects_dir();
            let found = scribe_core::transcript::iter_transcripts(&path).into_iter().find(|p| p.file_stem().map(|s| s == id.as_str()).unwrap_or(false));
            println!("transcript: {found:?}");
            if let Some(p) = found {
                let s = adapters::load_path(&p);
                println!("rounds={} title={:?} state={:?}", s.rounds.len(), s.title, scribe_core::build::turn_state(&scribe_core::transcript::read_all(&p), "").phase);
            }
        }
        _ => {
            eprintln!("usage: scribe-core list | render <id> | json <id> | build <id> | archive | sync [--force] | search <words>");
        }
    }
}

fn uuid_v4() -> String {
    // Random enough for a session id: time, pid and a hash mixed into the v4 layout.
    let mut bytes = [0u8; 16];
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut x = t as u64 ^ ((t >> 64) as u64).rotate_left(17) ^ (std::process::id() as u64).rotate_left(32);
    for b in bytes.iter_mut() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = (x & 0xff) as u8;
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let h: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}{}{}{}-{}{}-{}{}-{}{}-{}{}{}{}{}{}", h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], h[8], h[9], h[10], h[11], h[12], h[13], h[14], h[15])
}

fn resolve(target: &str, cfg: &Config) -> PathBuf {
    let p = PathBuf::from(target);
    if p.exists() {
        return p;
    }
    adapters::index_all(0, &cfg.agents)
        .into_iter()
        .find(|r| r.session_id == target || r.session_id.starts_with(target))
        .map(|r| r.path)
        .unwrap_or_else(|| panic!("no session {target}"))
}
