# Upload script for Trunk Recorder Pro

Runs a script of yours on each call [Trunk Recorder Pro](https://github.com/TrunkRecorder/trunk-recorder-pro)
records: to copy it to a server, upload it somewhere no plugin does, or
anything else. It does what Trunk Recorder's `uploadScript` setting does, and
existing scripts work unchanged.

## Settings

| Setting | |
|---|---|
| **Script** | What to run for each call: the full path of a script or program, then any arguments of your own. |
| **Time limit (seconds)** | A run that takes longer is stopped, and counts as failed. 300 by default. |

For each system:

| Setting | |
|---|---|
| **Script** | This system's own script, run instead of the one above. |

Quote arguments with spaces (`/home/me/up.sh 'my server'`). A program on the
PATH can be named on its own (`rclone copy …`); a script needs its full path
(`~/` works), since the plugin doesn't run in Trunk Recorder's folder. Scripts
have to be executable (`chmod +x`).

## What the script is given

After its own arguments, three paths:

1. the call's WAV,
2. its JSON (Trunk Recorder's call JSON),
3. its M4A.

The M4A is there when the recorder has an M4A encoder (built into macOS;
install [ffmpeg](https://ffmpeg.org) elsewhere). Without one, the third path
is where it would be, and there's no file there.

The script runs in the recorder's capture folder.

## When it fails

A script that exits with status 0 has done its job. Any other status marks the
call failed, with the last line the script printed (on stderr, if it printed
anything there), and everything it printed goes to the plugin's log.

Exit with status **75** to have the call tried again later: after 10 seconds,
1 minute, 5 minutes and 15 minutes. Calls still waiting when recording stops
are kept, and run again when it starts. (Trunk Recorder never retries.)

## Coming from Trunk Recorder

Paste each system's `uploadScript` into its **Script** (or set one for all of
them), with the script's full path in place of `./`. The arguments are the
same, in the same order.

The call's files are never deleted after the script runs: there's no
`audioArchive` setting, so a script that wants the files gone deletes them.

## Building

```sh
cargo build --release
trunk-pro plugin run ./target/release/upload-script ~/TrunkRecorderPro --settings examples/settings.json
```

## License

GPL-3.0-or-later, like Trunk Recorder.
