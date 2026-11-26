# Changelog

## [0.1.5]

- **Short, stable reasons for failed calls.** Trunk Recorder Pro after 0.5.1 counts a plugin's calls by the
  reason it gives, on its card: a failed call is now "the script ended with exit status N" (or "was
  killed", or "asked to try again later") without the script's last line. What the script printed is in the
  log, as before.

## [0.1.4]

- Built with trunk-recorder-plugin 0.5.0, for Trunk Recorder Pro v0.2.0: systems are numbered
  from 0 there, conventional ones too, and the call record names every field the recorder writes.

## [0.1.3]

- A system's sites: Trunk Recorder Pro now has systems with several sites, and a
  call heard on more than one is saved once, in the system's folder. Scripts get
  `SHORT_NAME`, `SITE` (the site the call was kept from) and `TALKGROUP` in
  their environment, so one can tell sites apart without reading the JSON.
- Built with trunk-recorder-plugin 0.4.0.

## [0.1.2]

- A system can override the script and the time limit (a script set in the system's own settings still comes first).
- Built with trunk-recorder-plugin 0.3.0.

## [0.1.0]

- Runs a script on each recorded call with the call's WAV, JSON and M4A paths,
  as Trunk Recorder's `uploadScript` does; one script for every system, or
  one per system.
- Arguments of your own, quoted as a shell would.
- A time limit; failures report what the script printed last.
- Exit status 75 tries the call again later.
