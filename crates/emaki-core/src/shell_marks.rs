//! The shell terminal's shell, started so that its prompt marks itself
//! (OSC 133: `A` where the prompt begins, `B` where what is typed begins,
//! `C` when a command begins), as VS Code's terminal starts one. The
//! terminal panel edits what is typed as the composer's text is edited,
//! across every line of it, and the marks are how it knows where that
//! begins. Nothing else changes: each of the person's own startup files
//! runs as it would, and the marks are put round the prompt last.
//!
//! zsh reads its startup files from a folder of ours (`ZDOTDIR`), each of
//! which runs the person's own of the same name first; bash reads one
//! file of ours in place of its login files, which it runs as a login
//! shell would; fish runs one of ours after its own configuration. Any
//! other shell, and PowerShell on Windows, starts as it always did, with
//! no marks.

use crate::paths;
use std::path::{Path, PathBuf};

const ZSHENV: &str = r#"# Emaki's shell terminal: zsh reads this in place of the person's own,
# which runs first. Written by Emaki.
EMAKI_ZDOTDIR=$ZDOTDIR
ZDOTDIR=$EMAKI_USER_ZDOTDIR
[[ -f $ZDOTDIR/.zshenv ]] && builtin source $ZDOTDIR/.zshenv
EMAKI_USER_ZDOTDIR=$ZDOTDIR
ZDOTDIR=$EMAKI_ZDOTDIR
"#;

const ZPROFILE: &str = r#"# Emaki's shell terminal: the person's own .zprofile. Written by Emaki.
ZDOTDIR=$EMAKI_USER_ZDOTDIR
[[ -f $ZDOTDIR/.zprofile ]] && builtin source $ZDOTDIR/.zprofile
EMAKI_USER_ZDOTDIR=$ZDOTDIR
ZDOTDIR=$EMAKI_ZDOTDIR
"#;

const ZSHRC: &str = r#"# Emaki's shell terminal: the person's own .zshrc, then the prompt's
# marks. Written by Emaki.
# The system's zshrc names the history after the folder zsh started in,
# which is ours: the person's history is theirs.
[[ $HISTFILE == $EMAKI_ZDOTDIR/.zsh_history ]] && HISTFILE=$EMAKI_USER_ZDOTDIR/.zsh_history
ZDOTDIR=$EMAKI_USER_ZDOTDIR
[[ -f $ZDOTDIR/.zshrc ]] && builtin source $ZDOTDIR/.zshrc
EMAKI_USER_ZDOTDIR=$ZDOTDIR
ZDOTDIR=$EMAKI_ZDOTDIR
builtin source $EMAKI_ZDOTDIR/emaki.zsh
"#;

const ZLOGIN: &str = r#"# Emaki's shell terminal: the person's own .zlogin, and from here on the
# session is theirs. Written by Emaki.
ZDOTDIR=$EMAKI_USER_ZDOTDIR
[[ -f $ZDOTDIR/.zlogin ]] && builtin source $ZDOTDIR/.zlogin
[[ -z $EMAKI_HAD_ZDOTDIR && $ZDOTDIR == $HOME ]] && unset ZDOTDIR
unset EMAKI_ZDOTDIR EMAKI_USER_ZDOTDIR EMAKI_HAD_ZDOTDIR
"#;

const ZSH_MARKS: &str = r#"# The prompt's marks for Emaki's terminal panel (OSC 133). Put round the
# prompt at every prompt and after every other hook, so a theme that sets
# the prompt each time keeps them. Written by Emaki.
_emaki_marks() {
  local a=$'%{\e]133;A\a%}' b=$'%{\e]133;B\a%}'
  PS1=${PS1//$a/}
  PS1=${PS1//$b/}
  PS1=$a$PS1$b
}
_emaki_ran() { builtin print -n $'\e]133;C\a' }
autoload -Uz add-zsh-hook
add-zsh-hook precmd _emaki_marks
add-zsh-hook preexec _emaki_ran
"#;

const BASH: &str = r#"# Emaki's shell terminal: bash reads this in place of its login files,
# which run as a login shell runs them, then the prompt's marks (OSC 133).
# Written by Emaki.
[ -r /etc/profile ] && . /etc/profile
if [ -r ~/.bash_profile ]; then . ~/.bash_profile
elif [ -r ~/.bash_login ]; then . ~/.bash_login
elif [ -r ~/.profile ]; then . ~/.profile
fi
__emaki_marks() {
  local a='\[\e]133;A\a\]' b='\[\e]133;B\a\]'
  PS1=${PS1//"$a"/}
  PS1=${PS1//"$b"/}
  PS1="$a$PS1$b"
}
PROMPT_COMMAND="${PROMPT_COMMAND:+$PROMPT_COMMAND
}__emaki_marks"
"#;

const FISH: &str = r#"# The prompt's marks for Emaki's terminal panel (OSC 133), round the
# person's own prompt. Written by Emaki.
functions -q fish_prompt; and functions -c fish_prompt __emaki_fish_prompt
function fish_prompt
    printf '\e]133;A\a'
    functions -q __emaki_fish_prompt; and __emaki_fish_prompt
    printf '\e]133;B\a'
end
function __emaki_ran --on-event fish_preexec
    printf '\e]133;C\a'
end
"#;

/// A file of ours, written when it is not already as it should be.
fn put(path: &Path, text: &str) -> Option<()> {
    if std::fs::read_to_string(path).ok().as_deref() != Some(text) {
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(path, text).ok()?;
    }
    Some(())
}

fn folder() -> PathBuf {
    paths::root().join("shell")
}

/// The shell to start in the terminal panel, as its arguments and what it
/// adds to the environment, so that its prompt marks itself. A shell we
/// do not know, or files that could not be written, start it as before.
pub fn shell_command() -> (Vec<String>, Vec<(String, String)>) {
    let plain = (crate::pty::shell_argv(), Vec::new());
    if cfg!(windows) {
        return plain;
    }
    let shell = plain.0[0].clone();
    let name = Path::new(&shell).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let dir = folder();
    match name.as_str() {
        "zsh" => {
            let z = dir.join("zsh");
            let wrote = [(".zshenv", ZSHENV), (".zprofile", ZPROFILE), (".zshrc", ZSHRC), (".zlogin", ZLOGIN), ("emaki.zsh", ZSH_MARKS)].iter().all(|(f, t)| put(&z.join(f), t).is_some());
            if !wrote {
                return plain;
            }
            let theirs = std::env::var("ZDOTDIR").ok().filter(|d| !d.is_empty());
            let home = std::env::var("HOME").unwrap_or_default();
            let mut env = vec![("ZDOTDIR".to_string(), z.to_string_lossy().to_string()), ("EMAKI_USER_ZDOTDIR".to_string(), theirs.clone().unwrap_or(home))];
            if theirs.is_some() {
                env.push(("EMAKI_HAD_ZDOTDIR".into(), "1".into()));
            }
            (plain.0, env)
        }
        "bash" => {
            let f = dir.join("emaki.bash");
            match put(&f, BASH) {
                Some(()) => (vec![shell, "--init-file".into(), f.to_string_lossy().to_string()], Vec::new()),
                None => plain,
            }
        }
        "fish" => {
            let f = dir.join("emaki.fish");
            match put(&f, FISH) {
                Some(()) => (vec![shell, "-l".into(), "--init-command".into(), format!("source '{}'", f.to_string_lossy().replace('\'', "\\'"))], Vec::new()),
                None => plain,
            }
        }
        _ => plain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each file a shell reads is one it can read: a slip in one would
    /// break every shell the panel opens. Checked by each shell that is
    /// here (`-n`, read without running).
    #[test]
    fn each_shell_reads_its_files() {
        let dir = std::env::temp_dir().join(format!("emaki-shell-marks-{}", std::process::id()));
        let files: [(&str, &str, &str); 7] = [
            ("zsh", "zshenv", ZSHENV),
            ("zsh", "zprofile", ZPROFILE),
            ("zsh", "zshrc", ZSHRC),
            ("zsh", "zlogin", ZLOGIN),
            ("zsh", "emaki.zsh", ZSH_MARKS),
            ("bash", "emaki.bash", BASH),
            ("fish", "emaki.fish", FISH),
        ];
        for (shell, name, text) in files {
            let path = dir.join(name);
            put(&path, text).expect("written");
            let Ok(out) = crate::process::command(shell).arg("-n").arg(&path).output() else { continue };
            assert!(out.status.success(), "{shell} -n {name}: {}", String::from_utf8_lossy(&out.stderr));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
