---
name: write-a-plugin
title: Write an audio plugin
when: The person wants an effect or instrument that does not exist ("a tape stop", "a bitcrusher with a sample-rate knob", "my own synth"), built in Rust on the ryolune plugin SDK.
---
# Write an audio plugin

Plugins are Rust crates on the `ryolune-plugin` SDK, built with cargo and loaded live without
restarting. Building needs the person's permission (Settings › Agent › plugins, or the Plugins
window's Build): a refusal says so; ask them to allow it rather than working around it.

## Steps

1. `plugin.guide`: the SDK, the kinds, `plugin.toml`, the audio-thread rules and an example.
   Read it before writing code.
2. `plugin.toolchain`: cargo and rustc must be installed (it says how otherwise).
3. `plugin.new name="Warm Drive" kind=effect` (or `instrument`): a working template crate with
   its tests, in `~/.lsuite/plugins-src/ryolune/<crate>/`.
4. Write the code with `plugin.writeSource name=… path=src/lib.rs contents=…` (whole files).
   Audio-thread rules: no allocation, locking, I/O or logging in `process`; parameters are
   smoothed; denormals and NaN guarded; state is plain data.
5. `plugin.build name=…`: compiler errors come back as `{file, line, message}`; fix and build
   again until it builds without warnings.
6. `plugin.publishLocal name=…`: installs the bundle and loads it; it is in `plugin.list` at
   once and songs using it reload.
7. Try it on a real track: `strip.setPlugin trackId=… plugin="Warm Drive" slot=…`, set its
   parameters with `strip.setParameter`.

## Checks

- `harness.measure trackId=… ` with the plugin bypassed (`strip.setBypass bypassed=true`) and
  then active: the numbers change the way the plugin should (a drive raises `highMids` and
  peaks, a filter removes `air`); no NaN error, no clipping at default settings.
- `harness.look view=mix trackId=…`: the waveform and spectrum show the effect.
- Report the plugin's name, kind, parameters and where its crate lives.
