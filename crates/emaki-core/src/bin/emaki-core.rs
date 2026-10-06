//! A small CLI over the core: list the index, render one session, sweep the
//! archive, sync search. It exists so the core can be checked without a window
//! and driven from a shell or a scheduled task.

use std::path::PathBuf;

use emaki_core::{adapters, archive, config::Config, redact::Redactor, render_md, search, store};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let cfg = Config::load();
    match cmd {
        "list" => {
            let refs = adapters::index_all(200, &cfg.agents);
            for r in refs.iter().filter(|r| !r.blank) {
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
        "shells" => {
            let target = args.get(1).expect("shells <session-id|path>");
            let session = emaki_core::build::build_from_path(&resolve(target, &cfg), "");
            for sh in &session.shells {
                println!("{}  {}  {}  {}  {}", sh.id, sh.started, if sh.ended.is_empty() { "running" } else { &sh.status }, sh.ended, if sh.description.is_empty() { &sh.command } else { &sh.description });
            }
        }
        "outline" => {
            let target = args.get(1).expect("outline <session-id|path>");
            let session = emaki_core::build::build_from_path(&resolve(target, &cfg), "");
            let entries = emaki_core::outline::of(&session);
            if args.get(2).map(String::as_str) == Some("--summarize") {
                // What the window would ask a small model, and how long it takes.
                let asked: Vec<String> = entries.iter().filter(|e| e.wants_summary()).map(|e| e.prompt.clone()).collect();
                let began = std::time::Instant::now();
                let lines = emaki_core::outline::summarize(&cfg.explain, &asked);
                let took = began.elapsed();
                let mut lines = lines.into_iter();
                for e in &entries {
                    let line = if e.wants_summary() { lines.next().flatten().unwrap_or_else(|| format!("(nothing came back) {}", e.title)) } else { e.title.clone() };
                    println!("{:>4}  {}  {}", e.round + 1, if e.wants_summary() { "*" } else { " " }, line);
                }
                println!("{} of {} entries asked about, {:.1}s", asked.len(), entries.len(), took.as_secs_f32());
                return;
            }
            for e in entries {
                println!("{:>4}  {:?}  {}\n      {}", e.round + 1, e.kind, e.title, e.gist);
            }
        }
        "git" => {
            let folder = std::fs::canonicalize(args.get(1).expect("git <folder> [path...]")).expect("a folder");
            println!("{:?}", emaki_core::git::branches(&folder));
            match emaki_core::git::status(&folder) {
                None => println!("not in a repository"),
                Some(st) => {
                    for p in &args[2..] {
                        let path = folder.join(p);
                        println!("{p}  {:?}", st.mark(&path, path.is_dir()));
                    }
                    if args.len() < 3 {
                        println!("{st:#?}");
                    }
                }
            }
        }
        "files" => {
            let folder = args.get(1).expect("files <folder> [typed]");
            let all = emaki_core::files::list(folder);
            let typed = args.get(2).map(String::as_str).unwrap_or("");
            for e in emaki_core::files::matches(&all, typed, 20) {
                println!("{}", e.path);
            }
            eprintln!("{} in all", all.len());
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
        "bench" => {
            // emaki-core bench [<session-id|path> ...]: where the time goes
            // when a session opens. With no argument, the five largest
            // transcripts on this machine. Each stage is timed on its own,
            // so a slow open can be blamed on parsing, building or rendering
            // instead of guessed at.
            let mut targets: Vec<PathBuf> = args[1..].iter().map(|t| resolve(t, &cfg)).collect();
            if targets.is_empty() {
                let mut refs = adapters::index_all(0, &cfg.agents);
                refs.sort_by(|a, b| b.size.cmp(&a.size));
                targets = refs.into_iter().take(5).map(|r| r.path).collect();
            }
            let redactor = Redactor::from_config(&cfg);
            println!("{:>9}  {:>7}  {:>7}  {:>7}  {:>7}  {:>6}  {}", "bytes", "read", "build", "render", "total", "rounds", "session");
            for path in targets {
                let t0 = std::time::Instant::now();
                let rows = emaki_core::transcript::read_all(&path);
                let t_read = t0.elapsed();
                let t1 = std::time::Instant::now();
                let mut session = adapters::load_path(&path);
                store::annotate(&mut session);
                let t_build = t1.elapsed();
                let t2 = std::time::Instant::now();
                let md = render_md::render(&session, &cfg, &redactor);
                let t_render = t2.elapsed();
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                println!(
                    "{:>9}  {:>6}ms  {:>6}ms  {:>6}ms  {:>6}ms  {:>6}  {} ({} rows, {} KB md)",
                    size,
                    t_read.as_millis(),
                    t_build.as_millis(),
                    t_render.as_millis(),
                    t0.elapsed().as_millis(),
                    session.rounds.len(),
                    path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                    rows.len(),
                    md.len() / 1024
                );
            }
        }
        "options" => {
            // What each agent says a session in this folder can be set to,
            // asked of the agent now: its modes, its models and the effort
            // levels each one takes.
            let cwd = args.get(1).cloned().unwrap_or_else(|| std::env::current_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default());
            for adapter in adapters::all() {
                let options = adapter.catalogue(&cwd).options;
                // Kept for the app's next launch, as the app keeps it.
                options.remember(adapter.id());
                if options.is_empty() {
                    eprintln!("{}: nothing to choose", adapter.id().as_str());
                    continue;
                }
                println!("{}", adapter.id().as_str());
                println!("  modes");
                for m in &options.modes {
                    println!("    {:<20} {:<22} {}", m.key, m.label, m.detail);
                }
                println!("  models");
                for m in &options.models {
                    let efforts: Vec<&str> = m.efforts.iter().map(|e| e.key.as_str()).collect();
                    println!("    {:<20} {:<22} {:<28} {}", m.key, m.label, m.resolved, efforts.join(" "));
                }
                println!("  efforts");
                for e in &options.efforts {
                    println!("    {:<20} {:<22} {}", e.key, e.label, e.detail);
                }
            }
        }
        "peers" => {
            // Every session with an inbox right now, and whether a message
            // could be delivered to it.
            let mut peers: Vec<_> = emaki_core::peer::registry().into_values().collect();
            peers.sort_by(|a, b| a.session_id.cmp(&b.session_id));
            for p in &peers {
                println!("{:<38} pid {:<7} {:<10} {:<8} {}", p.session_id, p.pid, p.kind, p.status, p.cwd);
            }
            eprintln!("{} sessions with an inbox", peers.len());
        }
        "inbox" => {
            // emaki-core inbox <session-id> <message...>: deliver into a
            // running terminal session.
            let sid = args.get(1).expect("inbox <session-id> <message>").clone();
            // `--again` sends the words as a repeat of the last ones.
            let again = args.get(2).is_some_and(|a| a == "--again");
            let text = args[if again { 3 } else { 2 }..].join(" ");
            let peers = emaki_core::peer::registry();
            let p = peers.get(&sid).expect("no inbox for that session");
            match emaki_core::peer::send(p, &text, again) {
                Ok(d) => println!("delivered {} status={} receipts={}", d.msg_id, d.status, d.receipts.len()),
                Err(e) => println!("refused: {e}"),
            }
        }
        "screen" => {
            // emaki-core screen < text: what a terminal's screen, piped
            // in, says the session is doing or asking.
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text);
            println!("working: {:?}", emaki_core::driver::working_on_screen(&text));
            println!("suggestion: {:?}", emaki_core::driver::suggestion_on_screen(&text));
            println!("dialog: {:#?}", emaki_core::driver::dialog_on_screen(&text));
        }
        "explain" => {
            // emaki-core explain <command...>: put one shell command into
            // plain words the way a permission card does, cached by content.
            // A probe of the model call, which the tests never make.
            let command = args[1..].join(" ");
            let mut input = serde_json::Map::new();
            input.insert("command".into(), serde_json::Value::String(command));
            let (tx, rx) = std::sync::mpsc::channel::<String>();
            let ex = emaki_core::explain::Explainer::new(cfg.explain.clone(), std::sync::Arc::new(move |_: &str, text: &str| {
                let _ = tx.send(text.to_string());
            }));
            let cached = !ex.lookup("Bash", &input).is_empty();
            ex.request("probe", "Bash", &input, true);
            match rx.recv_timeout(std::time::Duration::from_secs(cfg.explain.timeout_s + 5)) {
                Ok(text) if !text.is_empty() => println!("{text}\n  ({})", if cached { "from the cache" } else { "from the model" }),
                Ok(_) => println!("no explanation: the model gave nothing usable (is `claude` logged in?)"),
                Err(_) => println!("no explanation: timed out"),
            }
        }
        "statusline" => {
            // emaki-core statusline [install|restore]: what Claude Code's
            // status line is, or make it Emaki's script and back.
            use emaki_core::statusline::{self, State};
            let result = match args.get(1).map(String::as_str) {
                Some("install") => statusline::install(),
                Some("restore") => statusline::restore(),
                Some(other) => Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("statusline install | restore, not {other}"))),
                None => Ok(()),
            };
            if let Err(e) = result {
                eprintln!("{e}");
                std::process::exit(1);
            }
            match statusline::state() {
                State::Emaki { current: true } => println!("Emaki's script: {}", statusline::script_path().display()),
                State::Emaki { current: false } => println!("Emaki's script at {}, older than this build (install again to refresh it)", statusline::script_path().display()),
                State::Other(cmd) => println!("another command: {cmd}"),
                State::None => println!("none set in {}", statusline::settings_file().display()),
            }
        }
        "update" if args.get(1).map(String::as_str) == Some("install") => {
            // emaki-core update install <version> [<bundle>]: fetch that
            // release's installer and put it in place, over the named
            // bundle (macOS) or the running one. The app's own Update
            // button, from a terminal, for trying the swap on a copy.
            let version = args.get(2).expect("update install <version> [<bundle>]").clone();
            let bundle = args.get(3).map(PathBuf::from);
            let file = match emaki_core::update::download(&version, &|done, total| {
                if let Some(t) = total {
                    eprint!("\r{done} of {t} bytes");
                }
            }) {
                Ok(f) => f,
                Err(e) => {
                    println!("download failed: {e}");
                    std::process::exit(1);
                }
            };
            eprintln!("\nfetched {}", file.display());
            match emaki_core::update::install_into(&file, bundle.as_deref()) {
                Ok(()) => println!("installed {version}; the new app is starting"),
                Err(e) => {
                    println!("install failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        "update" => {
            // emaki-core update: what the newest release is, against this
            // build. A probe of the check the app makes once a day.
            let current = emaki_core::update::current_version();
            match emaki_core::update::latest() {
                Ok(latest) if emaki_core::update::is_newer(&latest, current) => println!("{latest} is available (this is {current}): {}", emaki_core::update::release_page(&latest)),
                Ok(latest) => println!("{current} is the newest release (GitHub has {latest})"),
                Err(e) => println!("could not check: {e}"),
            }
        }
        "pty" => {
            // emaki-core pty <cwd> [--resume <id>] [--secs <n>] [--keys <text>]... :
            // run an interactive Claude Code on a pty with no window,
            // send each `--keys` once the prompt is up (\r, \t, \e and
            // \Z for ⇧Tab are read), and print what the screen and the
            // registry say. A probe of the hidden terminal.
            let cwd = args.get(1).expect("pty <cwd> [--resume <id>] [--secs <n>] [--keys <text>]").clone();
            let mut resume = None;
            let mut secs = 12.0;
            let mut keys: Vec<String> = Vec::new();
            let mut it = args[2..].iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--resume" => resume = it.next().cloned(),
                    "--secs" => secs = it.next().and_then(|s| s.parse().ok()).unwrap_or(secs),
                    "--keys" => keys.extend(it.next().map(|k| k.replace("\\r", "\r").replace("\\t", "\t").replace("\\Z", "\x1b[Z").replace("\\e", "\x1b"))),
                    _ => {}
                }
            }
            let id = resume.clone().unwrap_or_else(uuid_v4);
            let argv = emaki_core::pty::claude_argv(&id, resume.is_some(), "", "");
            let started = std::time::Instant::now();
            let pty = emaki_core::pty::Pty::spawn(&argv, &cwd, std::sync::Arc::new(|| {})).expect("spawn");
            println!("session {id} pid {}", pty.pid);
            let modes = emaki_core::options::Options::cached(emaki_core::model::AgentId::ClaudeCode).modes;
            let mut said = String::new();
            let mut up = false;
            let mut sent_at = std::time::Instant::now();
            let mut keys = keys.into_iter();
            while started.elapsed().as_secs_f64() < secs && pty.alive() {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let peer = emaki_core::peer::registry_all().into_iter().find(|p| p.pid == pty.pid);
                let mode = emaki_core::driver::mode_on_screen(&pty.text(), &modes);
                let line = format!("registry {:?} mode {:?}", peer.as_ref().map(|p| p.status.clone()), mode);
                if line != said {
                    println!("{:>5.1}s {line}", started.elapsed().as_secs_f64());
                    said = line;
                }
                up |= mode.is_some() && peer.is_some();
                if up && sent_at.elapsed().as_millis() > 1500 {
                    if let Some(k) = keys.next() {
                        println!("{:>5.1}s keys {k:?}", started.elapsed().as_secs_f64());
                        pty.write(k.as_bytes());
                        sent_at = std::time::Instant::now();
                    }
                }
            }
            let styled = pty.styled();
            println!("---\n{}\n---", pty.text());
            println!("working: {:?}", emaki_core::driver::working_on_screen(&styled));
            println!("suggestion: {:?}", emaki_core::driver::suggestion_on_screen(&styled));
            println!("prompt: {:?}", emaki_core::driver::prompt_on_screen(&styled));
            println!("dialog: {:?}", emaki_core::driver::dialog_on_screen(&styled));
            pty.kill();
        }
        "drive" => {
            // emaki-core drive <cwd> <message...>: start a fresh headless
            // session, send one message, print events until the turn ends.
            let cwd = args.get(1).expect("drive <cwd> <message>").clone();
            let text = args[2..].join(" ");
            let id = format!("{}", uuid_v4());
            let (tx, rx) = std::sync::mpsc::channel();
            let started = std::time::Instant::now();
            let d = emaki_core::driver::Driver::start(&id, &cwd, false, "", "", tx).expect("start claude");
            println!("started {id} in {:.1}s; mode={} model={}", started.elapsed().as_secs_f64(), d.mode(), d.model());
            let caps = d.caps();
            println!("commands: {} slash: {} skills: {} tools: {}", caps.commands.len(), caps.slash_commands.len(), caps.skills.len(), caps.tools.len());
            d.send(&text, Vec::new()).expect("send");
            loop {
                match rx.recv_timeout(std::time::Duration::from_secs(180)) {
                    Ok(emaki_core::driver::Event::Permission(p)) => {
                        println!("permission: {} {}", p.tool_name, serde_json::to_string(&p.input).unwrap());
                        if p.is_question() {
                            // Answer every question with its first option, so the
                            // wire for answers can be checked from a terminal.
                            let mut answers = serde_json::Map::new();
                            for q in emaki_core::model::questions_of(&p.input) {
                                if let Some((label, _)) = q.options.first() {
                                    answers.insert(q.question.clone(), serde_json::Value::String(label.clone()));
                                }
                            }
                            println!("answering: {}", serde_json::to_string(&answers).unwrap());
                            d.answer_question(&p.request_id, answers);
                        } else {
                            d.answer_permission(&p.request_id, true, "");
                        }
                    }
                    Ok(emaki_core::driver::Event::Result(r)) => {
                        println!("result: {} error={} {}ms ${:.4}", r.subtype, r.is_error, r.duration_ms, r.cost_usd);
                        break;
                    }
                    Ok(emaki_core::driver::Event::Exit { code, error }) => {
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
            let path = emaki_core::paths::projects_dir();
            let found = emaki_core::transcript::iter_transcripts(&path).into_iter().find(|p| p.file_stem().map(|s| s == id.as_str()).unwrap_or(false));
            println!("transcript: {found:?}");
            if let Some(p) = found {
                let s = adapters::load_path(&p);
                println!("rounds={} title={:?} state={:?}", s.rounds.len(), s.title, emaki_core::build::turn_state(&emaki_core::transcript::read_all(&p), "").phase);
            }
        }
        _ => {
            eprintln!("usage: emaki-core list | render <id> | json <id> | build <id> | archive | sync [--force] | search <words> | bench [<id>...] | peers | inbox <id> <text> | explain <command> | update | statusline [install|restore] | drive <cwd> <text> | pty <cwd> [--resume <id>] [--secs <n>] [--keys <text>]");
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
