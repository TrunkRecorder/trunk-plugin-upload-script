# Changelog

## [Unreleased]

- A system can override the script and the time limit (a script set in the system's own settings still comes first).
- Built with trunk-recorder-plugin 0.3.0.

## [0.1.0]

- Runs a script on each recorded call with the call's WAV, JSON and M4A paths,
  as Trunk Recorder's `uploadScript` does; one script for every system, or
  one per system.
- Arguments of your own, quoted as a shell would.
- A time limit; failures report what the script printed last.
- Exit status 75 tries the call again later.
