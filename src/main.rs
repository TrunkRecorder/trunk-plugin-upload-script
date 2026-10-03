//! Upload script — a Trunk Recorder Pro plugin that runs a script of yours on
//! each recorded call, as Trunk Recorder's `uploadScript` does: the script
//! gets the call's WAV, JSON and M4A paths, in that order, after any
//! arguments of its own.

mod script;

use std::collections::HashMap;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use trunk_recorder_plugin::{format, topic, Attempt, CallQueue, ConcludedCall, Host, Manifest, Plugin, QueueOptions, Setup};

use script::Script;

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
struct Config {
    /// Script
    ///
    /// What to run for each call: the full path of a script or program, then any arguments of your own. It's given three more: the call's WAV, its JSON and its M4A. Systems with a script of their own run that instead.
    #[serde(alias = "uploadScript")]
    script: String,
    /// Time limit (seconds)
    ///
    /// A run that takes longer is stopped, and counts as failed.
    #[schemars(range(min = 1, max = 3600))]
    timeout_seconds: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config { script: String::new(), timeout_seconds: 300 }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", default)]
struct SystemConfig {
    /// Script
    ///
    /// This system's own script, instead of the one above.
    #[serde(alias = "uploadScript")]
    script: String,
}

struct UploadScript {
    queue: CallQueue,
}

impl Plugin for UploadScript {
    type Config = Config;
    type SystemConfig = SystemConfig;

    fn manifest() -> Manifest {
        Manifest {
            name: "Upload script".into(),
            subscribe: vec![topic::CALL_CONCLUDED.into()],
            audio_formats: vec![format::M4A.into()],
            ..trunk_recorder_plugin::manifest!()
        }
    }

    fn start(host: Host, setup: Setup<Config, SystemConfig>) -> Result<Self, String> {
        let timeout = Duration::from_secs(setup.config.timeout_seconds.clamp(1, 3600) as u64);
        let all = setup.config.script.trim();
        let all = if all.is_empty() { None } else { Some(Script::parse(all, timeout)?) };
        let mut scripts = HashMap::new();
        for s in &setup.systems {
            let own = s.config.as_ref().map_or("", |c| c.script.trim());
            let script = if own.is_empty() { all.clone() } else { Some(Script::parse(own, timeout).map_err(|e| format!("{}: {e}", s.short_name))?) };
            if let Some(script) = script {
                host.info(format!("{}: running {script}", s.short_name));
                scripts.insert(s.short_name.clone(), script);
            }
        }
        if scripts.is_empty() {
            return Err("Set the script to run.".into());
        }
        if !setup.has_format(format::M4A) {
            host.info("no M4A encoder: the M4A file scripts are given won't be there");
        }
        let cwd = setup.capture_dir.clone();
        let opts = QueueOptions { noun: "script run", ..QueueOptions::saved_in(&setup.data_dir) };
        let log = host.clone();
        let queue = CallQueue::start(host, opts, move |call: &ConcludedCall| {
            // By short name, a system's identity: a call saved for a later run still finds its system.
            let Some(script) = scripts.get(&call.call.short_name) else {
                return Attempt::Skip("no script for this system".into());
            };
            // (Where it would be, as Trunk Recorder passes it, when it couldn't be made.)
            let m4a = call.files.m4a.clone().unwrap_or_else(|| call.files.wav.with_extension("m4a"));
            script.run(&[&call.files.wav, &call.files.json, &m4a], &cwd, &log, &call.path)
        });
        Ok(UploadScript { queue })
    }

    fn call_concluded(&mut self, call: ConcludedCall) {
        self.queue.push(call);
    }

    fn shutdown(&mut self, grace: Duration) {
        self.queue.shutdown(grace);
    }
}

fn main() {
    trunk_recorder_plugin::run::<UploadScript>();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::path::{Path, PathBuf};
    use trunk_recorder_plugin::testing;
    use trunk_recorder_plugin::{HostMessage, Outcome, EXIT_CONFIG};

    /// An executable shell script in `dir`.
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn hello(dir: &Path, config: Value, system: Value) -> HostMessage {
        let mut h = testing::hello(dir, config);
        h.systems[0].config = system;
        HostMessage::Hello(h)
    }

    #[test]
    fn runs_with_the_calls_files_as_trunk_recorder_does() {
        let dir = testing::temp_dir("upload-script");
        let s = script(&dir, "up.sh", r#"printf '%s\n' "$@" "$(pwd)" > "$(dirname "$0")/args.txt""#);
        let call = testing::call(&dir, "sys1", 101);
        let cmd = format!("{} --to 'my server'", s.display());
        let out = testing::run::<UploadScript>([hello(&dir, json!({ "script": cmd }), Value::Null), HostMessage::CallConcluded(call.clone())]);
        assert!(out.ready(), "{:?}", out.messages);
        assert_eq!(out.results(), vec![(call.path.clone(), Outcome::Ok, String::new(), String::new())]);
        let args = std::fs::read_to_string(dir.join("args.txt")).unwrap();
        let cwd = std::fs::canonicalize(&dir).unwrap();
        let want = [
            "--to".to_string(),
            "my server".into(),
            call.files.wav.display().to_string(),
            call.files.json.display().to_string(),
            call.files.m4a.unwrap().display().to_string(),
            cwd.display().to_string(),
        ];
        assert_eq!(args.lines().collect::<Vec<_>>(), want);
    }

    #[test]
    fn a_failing_script_says_why() {
        let dir = testing::temp_dir("upload-script");
        let s = script(&dir, "up.sh", "echo working; echo 'scp: connection refused' >&2; exit 3");
        let out = testing::run::<UploadScript>([hello(&dir, json!({ "script": s }), Value::Null), HostMessage::CallConcluded(testing::call(&dir, "sys1", 5))]);
        let r = out.results();
        assert_eq!(r[0].1, Outcome::Failed);
        assert!(r[0].2.contains("exit status 3") && r[0].2.contains("connection refused"), "{}", r[0].2);
    }

    #[test]
    fn exit_75_tries_again_later() {
        let dir = testing::temp_dir("upload-script");
        let s = script(&dir, "up.sh", "exit 75");
        let out = testing::run::<UploadScript>([hello(&dir, json!({ "script": s }), Value::Null), HostMessage::CallConcluded(testing::call(&dir, "sys1", 5))]);
        assert!(out.results().is_empty(), "{:?}", out.results());
        assert_eq!(std::fs::read_to_string(dir.join("data/queue.jsonl")).unwrap().lines().count(), 1);
    }

    #[test]
    fn a_system_of_its_own_and_trunk_recorders_setting_name() {
        let dir = testing::temp_dir("upload-script");
        let ok = script(&dir, "ok.sh", "exit 0");
        let bad = script(&dir, "bad.sh", "exit 1");
        let out = testing::run::<UploadScript>([
            hello(&dir, json!({ "script": bad }), json!({ "uploadScript": ok })),
            HostMessage::CallConcluded(testing::call(&dir, "sys1", 5)),
        ]);
        assert_eq!(out.results()[0].1, Outcome::Ok);
    }

    #[test]
    fn a_script_that_takes_too_long_is_stopped() {
        let dir = testing::temp_dir("upload-script");
        let s = script(&dir, "up.sh", "sleep 30");
        let t0 = std::time::Instant::now();
        let out = testing::run::<UploadScript>([
            hello(&dir, json!({ "script": s, "timeoutSeconds": 1 }), Value::Null),
            HostMessage::CallConcluded(testing::call(&dir, "sys1", 5)),
        ]);
        assert!(t0.elapsed() < Duration::from_secs(10));
        let r = out.results();
        assert_eq!(r[0].1, Outcome::Failed);
        assert!(r[0].2.contains("1 s"), "{}", r[0].2);
    }

    #[test]
    fn bad_settings() {
        let dir = testing::temp_dir("upload-script");
        let plain = dir.join("plain.sh");
        std::fs::write(&plain, "#!/bin/sh\n").unwrap();
        for (config, why) in [
            (json!({}), "Set the script"),
            (json!({ "script": "./encode-upload.sh" }), "full path"),
            (json!({ "script": "/no/such/script.sh" }), "isn't there"),
            (json!({ "script": plain }), "chmod +x"),
            (json!({ "script": "/bin/echo 'unclosed" }), "quote"),
        ] {
            let out = testing::run::<UploadScript>([hello(&dir, config, Value::Null)]);
            assert_eq!(out.exit_code, EXIT_CONFIG);
            let msg = out.status().unwrap().1;
            assert!(msg.contains(why), "{msg}");
        }
    }
}
