# ldactl

A simple utility for accessing LaunchDarkly Relay AutoConfig Streams (Enterprise only).

## Usage

```
LaunchDarkly Relay AutoConfig CLI

Usage: ldactl [OPTIONS] --credential <CREDENTIAL> [-- <EXEC_ARGS>...]

Arguments:
  [EXEC_ARGS]...

Options:
  -k, --credential <CREDENTIAL>  [env: LD_RELAY_AUTO_CONFIG_KEY=]
  -u, --stream-uri <URI>         [env: LD_STREAM_URI=] [default: https://stream.launchdarkly.com/]
  -o, --once
  -f, --output-file <OUT_FILE>   [env: LD_AUTO_CONFIG_OUTPUT_FILE=]
  -e, --exec <EXEC>
  -h, --help                     Print help (see more with '--help')
```

## Key features

- Atomically write all environment configurations (SDK keys, mobile keys, etc) to a JSON file when updates are received
- Execute a hook command for every change event (insert, update, delete). Hooks will receive the payload via JSON on STDIN
- Execute once with `--once` instead of subscribing for one-off updates

## Use cases

- Sync LaunchDarkly credentiuals to third-party services such as AWS secrets
- Render configuration files and restart services when SDK keys are rotated

### Examples

#### Executing `jq`

![executing jq](./assets/hook-screenshot.png)

#### Writing to a file

![writing to file](./assets/file-screenshot.png)
