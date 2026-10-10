//! The coding agents a machine can have: which there are, whether each is
//! installed here, and how to get one and sign in to it.
//!
//! This is wider than `adapters`, which is the agents whose sessions are
//! read. An agent listed here with no adapter is one the window can say
//! is installed and nothing more (`Agent::reads`).
//!
//! Looking is cheap and asks nothing of a model: a file by its name in
//! the folders a shell would search, the program's own `--version`, and
//! whether the files a sign-in leaves are there. No file of an agent's is
//! opened, a credential least of all.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::model::AgentId;
use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Os {
    Mac,
    Linux,
    Windows,
}

impl Os {
    pub const ALL: [Os; 3] = [Os::Mac, Os::Linux, Os::Windows];

    /// The system this build runs on.
    pub fn here() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Linux
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Os::Mac => "mac",
            Os::Linux => "linux",
            Os::Windows => "windows",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Os::Mac => "macOS",
            Os::Linux => "Linux",
            Os::Windows => "Windows",
        }
    }

    pub fn parse(s: &str) -> Option<Os> {
        Os::ALL.into_iter().find(|o| o.as_str() == s)
    }
}

const UNIX: &[Os] = &[Os::Mac, Os::Linux];
const MAC: &[Os] = &[Os::Mac];
const WINDOWS: &[Os] = &[Os::Windows];
const ANY: &[Os] = &[Os::Mac, Os::Linux, Os::Windows];

/// One way to install an agent, as its maker's documentation gives it.
#[derive(Debug, Clone, Copy)]
pub struct Way {
    pub os: &'static [Os],
    /// What does the installing: "Script", "npm", "Homebrew".
    pub by: &'static str,
    pub command: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Agent {
    pub id: &'static str,
    pub name: &'static str,
    pub maker: &'static str,
    /// One line on what it is.
    pub about: &'static str,
    /// The program's name on `PATH`, the first the one to type.
    pub bins: &'static [&'static str],
    /// The first way that fits a system is the one its maker recommends.
    pub install: &'static [Way],
    /// What to type to sign in.
    pub sign_in: &'static str,
    /// What happens then.
    pub sign_in_how: &'static str,
    /// What account it takes.
    pub plans: &'static str,
    /// The variables that carry a key, for signing in without a browser.
    pub key_env: &'static [&'static str],
    /// Where it keeps its settings.
    pub home: &'static str,
    /// Files a sign-in leaves. One there says someone signed in; none
    /// there says nothing, since most agents can keep a sign-in in the
    /// system's keyring instead.
    pub signed: &'static [&'static str],
    /// On a Mac, the keychain item a sign-in leaves, or empty.
    pub keychain: &'static str,
    /// What it is asked its version with.
    pub version_arg: &'static str,
    pub site: &'static str,
    pub docs: &'static str,
    /// The adapter that reads its sessions, when there is one.
    pub reads: Option<AgentId>,
}

impl Agent {
    /// The ways to install it on `os`, the recommended one first.
    pub fn ways(&self, os: Os) -> Vec<&'static Way> {
        self.install.iter().filter(|w| w.os.contains(&os)).collect()
    }

    /// The program to type.
    pub fn bin(&self) -> &'static str {
        self.bins.first().copied().unwrap_or("")
    }
}

pub fn all() -> &'static [Agent] {
    ALL
}

pub fn by_id(id: &str) -> Option<&'static Agent> {
    ALL.iter().find(|a| a.id == id)
}

/// The catalogue's entry for an agent whose sessions are read.
pub fn of(agent: AgentId) -> Option<&'static Agent> {
    ALL.iter().find(|a| a.reads == Some(agent))
}

const ALL: &[Agent] = &[
    Agent {
        id: "claude-code",
        name: "Claude Code",
        maker: "Anthropic",
        about: "Anthropic's agent for the terminal. It reads a project, edits files and runs commands, with Claude's models.",
        bins: &["claude"],
        install: &[
            Way { os: UNIX, by: "Script", command: "curl -fsSL https://claude.ai/install.sh | bash" },
            Way { os: MAC, by: "Homebrew", command: "brew install --cask claude-code" },
            Way { os: WINDOWS, by: "PowerShell", command: "irm https://claude.ai/install.ps1 | iex" },
            Way { os: WINDOWS, by: "winget", command: "winget install Anthropic.ClaudeCode" },
            Way { os: ANY, by: "npm (needs Node.js 22 or later)", command: "npm install -g @anthropic-ai/claude-code" },
        ],
        sign_in: "claude",
        sign_in_how: "The first run opens a browser to sign in. Later, type /login in a session to sign in again or change account.",
        plans: "Takes a Claude Pro, Max, Team or Enterprise plan, or a Console account billed by use. The free plan does not include it.",
        key_env: &["ANTHROPIC_API_KEY"],
        home: "~/.claude",
        signed: &["~/.claude/.credentials.json"],
        keychain: "Claude Code-credentials",
        version_arg: "--version",
        site: "https://claude.com/product/claude-code",
        docs: "https://code.claude.com/docs",
        reads: Some(AgentId::ClaudeCode),
    },
    Agent {
        id: "codex",
        name: "Codex",
        maker: "OpenAI",
        about: "OpenAI's coding agent for the terminal, with the GPT models a ChatGPT plan includes.",
        bins: &["codex"],
        install: &[
            Way { os: UNIX, by: "Script", command: "curl -fsSL https://chatgpt.com/codex/install.sh | sh" },
            Way { os: MAC, by: "Homebrew", command: "brew install --cask codex" },
            Way { os: WINDOWS, by: "PowerShell", command: "powershell -ExecutionPolicy ByPass -c \"irm https://chatgpt.com/codex/install.ps1 | iex\"" },
            Way { os: ANY, by: "npm (needs Node.js)", command: "npm install -g @openai/codex" },
        ],
        sign_in: "codex login",
        sign_in_how: "A browser opens to sign in with ChatGPT. On a machine with no browser, codex login --device-auth gives a code to enter on another.",
        plans: "Takes a ChatGPT Plus, Pro, Business, Edu or Enterprise plan, or an OpenAI API key.",
        key_env: &["OPENAI_API_KEY"],
        home: "~/.codex",
        signed: &["~/.codex/auth.json"],
        keychain: "",
        version_arg: "--version",
        site: "https://developers.openai.com/codex",
        docs: "https://developers.openai.com/codex/cli",
        reads: Some(AgentId::Codex),
    },
    Agent {
        id: "gemini",
        name: "Gemini CLI",
        maker: "Google",
        about: "Google's open-source agent for the terminal, with the Gemini models.",
        bins: &["gemini"],
        install: &[
            Way { os: ANY, by: "npm (needs Node.js)", command: "npm install -g @google/gemini-cli" },
            Way { os: MAC, by: "Homebrew", command: "brew install gemini-cli" },
        ],
        sign_in: "gemini",
        sign_in_how: "The first run asks how to sign in. Choose Sign in with Google and a browser opens.",
        plans: "Takes a Google account with a paid Gemini plan, or a Gemini API key. Google's documentation says accounts on the unpaid tier were moved to its Antigravity CLI in June 2026.",
        key_env: &["GEMINI_API_KEY"],
        home: "~/.gemini",
        signed: &["~/.gemini/oauth_creds.json"],
        keychain: "",
        version_arg: "--version",
        site: "https://geminicli.com",
        docs: "https://geminicli.com/docs",
        reads: None,
    },
    Agent {
        id: "copilot",
        name: "Copilot CLI",
        maker: "GitHub",
        about: "GitHub Copilot as an agent in the terminal, with the models a Copilot plan offers and GitHub close at hand.",
        bins: &["copilot"],
        install: &[
            Way { os: UNIX, by: "Script", command: "curl -fsSL https://gh.io/copilot-install | bash" },
            Way { os: MAC, by: "Homebrew", command: "brew install --cask copilot-cli" },
            Way { os: WINDOWS, by: "winget", command: "winget install GitHub.Copilot" },
            Way { os: ANY, by: "npm (needs Node.js 22 or later)", command: "npm install -g @github/copilot" },
        ],
        sign_in: "copilot login",
        sign_in_how: "A code is shown to enter on github.com. Inside a session, /login does the same.",
        plans: "Takes a GitHub account with an active Copilot subscription.",
        key_env: &["COPILOT_GITHUB_TOKEN"],
        home: "~/.copilot",
        signed: &[],
        keychain: "copilot-cli",
        version_arg: "--version",
        site: "https://github.com/features/copilot/cli",
        docs: "https://docs.github.com/en/copilot/how-tos/copilot-cli",
        reads: None,
    },
    Agent {
        id: "cursor",
        name: "Cursor CLI",
        maker: "Cursor",
        about: "The agent of the Cursor editor, run from a terminal with no editor open.",
        bins: &["cursor-agent"],
        install: &[
            Way { os: UNIX, by: "Script", command: "curl https://cursor.com/install -fsS | bash" },
            Way { os: WINDOWS, by: "PowerShell", command: "irm 'https://cursor.com/install?win32=true' | iex" },
        ],
        sign_in: "cursor-agent login",
        sign_in_how: "A browser opens to sign in to Cursor. cursor-agent status says who is signed in.",
        plans: "Takes a Cursor account, or a Cursor API key.",
        key_env: &["CURSOR_API_KEY"],
        home: "~/.cursor",
        signed: &["~/.cursor/auth.json"],
        keychain: "cursor-access-token",
        version_arg: "--version",
        site: "https://cursor.com/cli",
        docs: "https://cursor.com/docs/cli",
        reads: None,
    },
    Agent {
        id: "opencode",
        name: "opencode",
        maker: "Anomaly",
        about: "An open-source agent for the terminal that works with many model providers, a ChatGPT or Copilot plan among them.",
        bins: &["opencode"],
        install: &[
            Way { os: UNIX, by: "Script", command: "curl -fsSL https://opencode.ai/install | bash" },
            Way { os: MAC, by: "Homebrew", command: "brew install anomalyco/tap/opencode" },
            Way { os: WINDOWS, by: "Scoop", command: "scoop install opencode" },
            Way { os: WINDOWS, by: "Chocolatey", command: "choco install opencode" },
            Way { os: ANY, by: "npm (needs Node.js)", command: "npm install -g opencode-ai" },
        ],
        sign_in: "opencode",
        sign_in_how: "In a session, type /connect and choose a provider. opencode auth list says which are connected.",
        plans: "Takes a ChatGPT Plus or Pro plan, a GitHub Copilot plan, opencode's own Zen, or a provider's API key.",
        key_env: &[],
        home: "~/.config/opencode",
        signed: &["~/.local/share/opencode/auth.json"],
        keychain: "",
        version_arg: "--version",
        site: "https://opencode.ai",
        docs: "https://opencode.ai/docs",
        reads: None,
    },
    Agent {
        id: "amp",
        name: "Amp",
        maker: "Amp",
        about: "An agent for the terminal that picks its own models, with threads kept on Amp's server to share.",
        bins: &["amp"],
        install: &[Way { os: UNIX, by: "Script", command: "curl -fsSL https://ampcode.com/install.sh | bash" }],
        sign_in: "amp login",
        sign_in_how: "A browser opens to sign in to Amp.",
        plans: "Takes an Amp account: a free tier, a monthly plan, or credits paid for by use.",
        key_env: &["AMP_API_KEY"],
        home: "~/.config/amp",
        signed: &[],
        keychain: "",
        version_arg: "version",
        site: "https://ampcode.com",
        docs: "https://ampcode.com/docs/cli",
        reads: None,
    },
    Agent {
        id: "qwen",
        name: "Qwen Code",
        maker: "Alibaba",
        about: "Alibaba's open-source agent for the terminal, with the Qwen models.",
        bins: &["qwen"],
        install: &[
            Way { os: UNIX, by: "Script", command: "curl -fsSL https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/install-qwen-standalone.sh | bash" },
            Way { os: MAC, by: "Homebrew", command: "brew install qwen-code" },
            Way { os: ANY, by: "npm (needs Node.js 22 or later)", command: "npm install -g @qwen-code/qwen-code@latest" },
        ],
        sign_in: "qwen",
        sign_in_how: "In a session, type /auth and choose how to sign in.",
        plans: "Takes an Alibaba Cloud Coding Plan or an API key. The free tier by Qwen sign-in ended in April 2026.",
        key_env: &["DASHSCOPE_API_KEY", "BAILIAN_CODING_PLAN_API_KEY"],
        home: "~/.qwen",
        signed: &[],
        keychain: "",
        version_arg: "--version",
        site: "https://github.com/QwenLM/qwen-code",
        docs: "https://qwenlm.github.io/qwen-code-docs",
        reads: None,
    },
    Agent {
        id: "aider",
        name: "Aider",
        maker: "Aider",
        about: "Open-source pair programming in the terminal. It edits the files of a git repository and commits as it goes, with a model of any provider.",
        bins: &["aider"],
        install: &[
            Way { os: UNIX, by: "Script", command: "curl -LsSf https://aider.chat/install.sh | sh" },
            Way { os: WINDOWS, by: "PowerShell", command: "powershell -ExecutionPolicy ByPass -c \"irm https://aider.chat/install.ps1 | iex\"" },
            Way { os: ANY, by: "pipx (needs Python)", command: "pipx install aider-chat" },
        ],
        sign_in: "aider --api-key anthropic=<your key>",
        sign_in_how: "Aider has no account of its own. It is given a key of the model provider's: on the command line as here, in a .env file, or in the environment.",
        plans: "Takes an API key of a model provider, billed by use.",
        key_env: &["ANTHROPIC_API_KEY", "OPENAI_API_KEY"],
        home: "",
        signed: &[],
        keychain: "",
        version_arg: "--version",
        site: "https://aider.chat",
        docs: "https://aider.chat/docs",
        reads: None,
    },
    Agent {
        id: "goose",
        name: "Goose",
        maker: "Agentic AI Foundation",
        about: "An open-source agent, begun at Block, that runs on the machine with a model of any provider and extensions over MCP.",
        bins: &["goose"],
        install: &[
            Way { os: UNIX, by: "Script", command: "curl -fsSL https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh | bash" },
            Way { os: MAC, by: "Homebrew", command: "brew install block-goose-cli" },
            Way { os: WINDOWS, by: "PowerShell", command: "Invoke-WebRequest -Uri \"https://raw.githubusercontent.com/aaif-goose/goose/main/download_cli.ps1\" -OutFile \"download_cli.ps1\"; .\\download_cli.ps1" },
        ],
        sign_in: "goose configure",
        sign_in_how: "It asks for a model provider and that provider's key, and keeps the key in the system's keyring.",
        plans: "Takes an API key of a model provider, billed by use.",
        key_env: &[],
        home: "~/.config/goose",
        signed: &[],
        keychain: "",
        version_arg: "--version",
        site: "https://goose-docs.ai",
        docs: "https://goose-docs.ai/docs/getting-started/installation",
        reads: None,
    },
    Agent {
        id: "kiro",
        name: "Kiro CLI",
        maker: "Amazon Web Services",
        about: "AWS's agent for the terminal, which took the place of the Amazon Q Developer CLI.",
        bins: &["kiro-cli"],
        install: &[Way { os: UNIX, by: "Script", command: "curl -fsSL https://cli.kiro.dev/install | bash" }],
        sign_in: "kiro-cli login",
        sign_in_how: "A browser opens to sign in with Google, GitHub, an AWS Builder ID or an organization's identity. kiro-cli doctor checks the set-up.",
        plans: "Takes a Kiro plan, which includes the CLI.",
        key_env: &[],
        home: "~/.kiro",
        signed: &[],
        keychain: "",
        version_arg: "--version",
        site: "https://kiro.dev/cli",
        docs: "https://kiro.dev/docs/cli",
        reads: None,
    },
    Agent {
        id: "droid",
        name: "Droid",
        maker: "Factory",
        about: "Factory's agent for the terminal, which works with the models of several providers.",
        bins: &["droid"],
        install: &[
            Way { os: UNIX, by: "Script", command: "curl -fsSL https://app.factory.ai/cli | sh" },
            Way { os: WINDOWS, by: "PowerShell", command: "irm https://app.factory.ai/cli/windows | iex" },
            Way { os: ANY, by: "npm (needs Node.js)", command: "npm install -g droid" },
        ],
        sign_in: "droid",
        sign_in_how: "The first run opens a browser to sign in to Factory.",
        plans: "Takes a Factory account.",
        key_env: &["FACTORY_API_KEY"],
        home: "~/.factory",
        signed: &[],
        keychain: "",
        version_arg: "--version",
        site: "https://factory.ai",
        docs: "https://docs.factory.ai",
        reads: None,
    },
];

/// What looking for one agent found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// The program, when it is installed.
    pub path: Option<PathBuf>,
    /// What its `--version` says, as a dotted number; empty when it did
    /// not answer.
    pub version: String,
    /// Whether its settings folder is there. An editor's extension can
    /// leave one, so it does not say the program is installed.
    pub home: bool,
    /// Whether a sign-in was seen: one of the files it leaves, its item
    /// in the keychain, or a key in the environment. Not seen is not
    /// "signed out".
    pub signed: bool,
}

impl Found {
    pub fn installed(&self) -> bool {
        self.path.is_some()
    }
}

/// The folders a program is looked for in: `PATH`, then where installers
/// put things when `PATH` does not say. An app started from the Dock has
/// the system's `PATH` until the login shell's is asked for.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    let home = paths::home();
    for d in [".local/bin", ".claude/local", ".npm-global/bin", ".npm/bin", ".bun/bin", ".volta/bin", ".asdf/shims", ".local/share/mise/shims", ".cargo/bin", ".deno/bin", "Library/pnpm", ".opencode/bin", ".amp/bin"] {
        dirs.push(home.join(d));
    }
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            dirs.push(PathBuf::from(appdata).join("npm"));
        }
    } else {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}

/// The agent's program in `dirs`, the first there is.
pub fn find_in(agent: &Agent, dirs: &[PathBuf]) -> Option<PathBuf> {
    let suffixes: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ""] } else { &[""] };
    for dir in dirs {
        for bin in agent.bins {
            for suffix in suffixes {
                let p = dir.join(format!("{bin}{suffix}"));
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// The dotted number in what a `--version` printed: "2.1.3 (Claude
/// Code)", "codex-cli 0.50.0", "v1.4" all hold one.
pub fn version_from(text: &str) -> String {
    text.split(|c: char| c.is_whitespace() || matches!(c, '/' | ',' | '(' | ')' | '@'))
        .map(|t| t.trim_start_matches('v'))
        .find(|t| t.starts_with(|c: char| c.is_ascii_digit()) && t.contains('.'))
        .map(|t| t.trim_end_matches(|c: char| !c.is_ascii_alphanumeric()).to_string())
        .unwrap_or_default()
}

/// How long a program has to say its version. One written for Node
/// takes a second on a cold start.
const VERSION_WAIT: Duration = Duration::from_secs(6);

/// What `program` says when asked its version with `arg`, with `dirs` as its `PATH` (a script
/// with a `node` first line needs to find one). Empty when it does not
/// answer in time.
pub fn version_of(program: &Path, arg: &str, dirs: &[PathBuf]) -> String {
    let mut cmd = Command::new(program);
    cmd.arg(arg).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    // An agent asked from inside a session of another must not take
    // itself for that session's child.
    cmd.env_clear().envs(crate::driver::child_env());
    if let Ok(path) = std::env::join_paths(dirs) {
        cmd.env("PATH", path);
    }
    let Ok(mut child) = cmd.spawn() else { return String::new() };
    let Some(mut out) = child.stdout.take() else { return String::new() };
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut out, &mut text);
        text
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < VERSION_WAIT => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return String::new();
            }
        }
    }
    version_from(&reader.join().unwrap_or_default())
}

/// Whether a Mac's keychain holds an item of that name. Its attributes
/// are asked for and never its secret, so the system asks the person
/// nothing.
fn in_keychain(service: &str) -> bool {
    if !cfg!(target_os = "macos") || service.is_empty() {
        return false;
    }
    Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", service])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Whether a sign-in to an agent can be seen from outside.
pub fn signed_in(agent: &Agent) -> bool {
    agent.key_env.iter().any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty())) || agent.signed.iter().any(|f| paths::expand_tilde(f).is_file()) || in_keychain(agent.keychain)
}

/// One agent looked for in `dirs`.
pub fn detect_in(agent: &Agent, dirs: &[PathBuf]) -> Found {
    let path = find_in(agent, dirs);
    let home = !agent.home.is_empty() && paths::expand_tilde(agent.home).is_dir();
    let (version, signed) = match &path {
        Some(p) => (version_of(p, agent.version_arg, dirs), signed_in(agent)),
        None => (String::new(), false),
    };
    Found { path, version, home, signed }
}

/// Every agent in the catalogue looked for, each on a thread of its own
/// since each waits on a program: in the catalogue's order. Blocking for
/// as long as the slowest takes to say its version.
pub fn detect_all() -> Vec<(&'static str, Found)> {
    let dirs = search_dirs();
    std::thread::scope(|s| {
        let jobs: Vec<_> = ALL.iter().map(|a| (a.id, s.spawn(|| detect_in(a, &dirs)))).collect();
        jobs.into_iter().map(|(id, j)| (id, j.join().unwrap_or_default())).collect()
    })
}
