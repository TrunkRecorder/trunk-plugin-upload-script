//! A script and its arguments, as the user wrote it, and running it on a call.

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use trunk_recorder_plugin::{Attempt, Host};

/// The exit status that asks for the call to be tried again later (sysexits' EX_TEMPFAIL).
pub const TRY_AGAIN: i32 = 75;

#[derive(Clone, Debug)]
pub struct Script {
    program: PathBuf,
    args: Vec<String>,
    timeout: Duration,
    text: String,
}

impl fmt::Display for Script {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Script {
    /// `text`: a program and its arguments, split as a shell would (quotes and backslashes).
    pub fn parse(text: &str, timeout: Duration) -> Result<Script, String> {
        let mut words = split(text)?;
        if words.is_empty() {
            return Err("the script is empty".into());
        }
        let first = words.remove(0);
        let first = match (first.strip_prefix("~/"), std::env::var_os("HOME")) {
            (Some(rest), Some(home)) => Path::new(&home).join(rest),
            _ => PathBuf::from(&first),
        };
        let program = if first.components().count() > 1 {
            // Plugins don't run from Trunk Recorder's folder: a relative path would be a guess.
            if first.is_relative() {
                return Err(format!("give the script's full path, not {}", first.display()));
            }
            if !first.is_file() {
                return Err(format!("{} isn't there", first.display()));
            }
            if !executable(&first) {
                return Err(format!("{} can't be run: make it executable (chmod +x)", first.display()));
            }
            first
        } else {
            // A program on the PATH (python3, rclone, …).
            which(&first).ok_or_else(|| format!("there's no program called {} on this computer", first.display()))?
        };
        Ok(Script { program, args: words, timeout, text: text.to_string() })
    }

    /// Run it with `files` after its own arguments, in `cwd`; `call`: for the log.
    pub fn run(&self, files: &[&Path], cwd: &Path, log: &Host, call: &str) -> Attempt {
        let mut c = Command::new(&self.program);
        c.args(&self.args).args(files).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        // A group of its own, so what it starts can be stopped with it.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut c, 0);
        let mut child = match c.spawn() {
            Ok(ch) => ch,
            Err(e) => return Attempt::Fail(format!("couldn't run {}: {e}", self.program.display())),
        };
        // Read its output as it comes, so a chatty script can't fill the pipe and stall.
        let read = |r: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut s = String::new();
                if let Some(mut r) = r {
                    let _ = r.read_to_string(&mut s);
                }
                s
            })
        };
        let out = read(child.stdout.take().map(|o| Box::new(o) as Box<dyn Read + Send>));
        let err = read(child.stderr.take().map(|e| Box::new(e) as Box<dyn Read + Send>));
        let t0 = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) if t0.elapsed() >= self.timeout => {
                    #[cfg(unix)]
                    let _ = Command::new("kill").args(["-KILL", "--", &format!("-{}", child.id())]).stderr(Stdio::null()).status();
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => return Attempt::Fail(e.to_string()),
            }
        };
        let Some(status) = status else {
            // (Not waiting for its output: something it started may still hold it open.)
            return Attempt::Fail(format!("took longer than {} s, so it was stopped", self.timeout.as_secs()));
        };
        let (out, err) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
        if status.success() {
            for l in out.lines().chain(err.lines()).filter(|l| !l.trim().is_empty()) {
                log.debug(format!("{call}: {l}"));
            }
            return Attempt::Done { url: String::new() };
        }
        for l in out.lines().chain(err.lines()).filter(|l| !l.trim().is_empty()) {
            log.warn(format!("{call}: {l}"));
        }
        // What it said last, on stderr if anything.
        let last = |s: &str| s.lines().rev().find(|l| !l.trim().is_empty()).map(|l| l.trim().to_string());
        let said = last(&err).or_else(|| last(&out)).unwrap_or_default();
        let said = if said.is_empty() { String::new() } else { format!(": {}", said.chars().take(200).collect::<String>()) };
        match status.code() {
            Some(TRY_AGAIN) => Attempt::Retry(format!("the script asked to try again later{said}")),
            Some(code) => Attempt::Fail(format!("the script ended with exit status {code}{said}")),
            None => Attempt::Fail(format!("the script was killed{said}")),
        }
    }
}

/// Split a command line into words: whitespace between them, '…' and "…"
/// for words with spaces, \ for the next character (not in '…'). The same
/// rules as Trunk Recorder's `uploadScript`.
pub fn split(text: &str) -> Result<Vec<String>, String> {
    let (mut words, mut cur) = (Vec::new(), String::new());
    let (mut single, mut double, mut escaped, mut started) = (false, false, false, false);
    for c in text.chars() {
        if escaped {
            cur.push(c);
            escaped = false;
        } else if c == '\\' && !single {
            escaped = true;
            started = true;
        } else if c == '\'' && !double {
            single = !single;
            started = true;
        } else if c == '"' && !single {
            double = !double;
            started = true;
        } else if c.is_whitespace() && !single && !double {
            if started {
                words.push(std::mem::take(&mut cur));
                started = false;
            }
        } else {
            cur.push(c);
            started = true;
        }
    }
    if escaped {
        return Err("the script ends with a backslash".into());
    }
    if single || double {
        return Err("the script has a quote that isn't closed".into());
    }
    if started {
        words.push(cur);
    }
    Ok(words)
}

#[cfg(unix)]
fn executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(_: &Path) -> bool {
    true
}

/// A program on the PATH — or where package managers put them, since an app
/// started from the desktop doesn't get the shell's PATH.
fn which(name: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if cfg!(unix) {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
    }
    let names: Vec<PathBuf> = if cfg!(windows) && name.extension().is_none() {
        ["exe", "bat", "cmd"].iter().map(|e| name.with_extension(e)).collect()
    } else {
        vec![name.to_path_buf()]
    };
    dirs.iter().flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_trunk_recorder() {
        assert_eq!(split("/a/b.sh").unwrap(), ["/a/b.sh"]);
        assert_eq!(split("  /a/b.sh  --x 1 ").unwrap(), ["/a/b.sh", "--x", "1"]);
        assert_eq!(split(r#"/a/b.sh 'one two' "three four" five\ six"#).unwrap(), ["/a/b.sh", "one two", "three four", "five six"]);
        assert_eq!(split(r#"x 'it\s' "a\"b" """#).unwrap(), ["x", r"it\s", "a\"b", ""]);
        assert!(split("x 'open").is_err());
        assert!(split("x \\").is_err());
    }

    #[test]
    fn programs_on_the_path() {
        let s = Script::parse("sh -c true", Duration::from_secs(1)).unwrap();
        assert!(s.program.is_absolute());
        assert_eq!(s.args, ["-c", "true"]);
        assert!(Script::parse("no-such-program-anywhere", Duration::from_secs(1)).unwrap_err().contains("no program"));
    }
}
