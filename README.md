[![CI](https://github.com/foriequal0/tpmiddle-rs/workflows/CI/badge.svg?branch=master&event=push)](https://github.com/foriequal0/tpmiddle-rs/actions?query=workflow%3ACI+event%3Apush+branch%3Amaster)

tpmiddle-rs
===========

`tpmiddle-rs` is a Rust port of [tene0/tpmiddle](https://github.com/tene0/tpmiddle) with some improvements.

## Features

### Lightweight

It doesn't need ThinkPad TrackPoint Keyboard software since it directly communicates with the keyboard.
You only need a `tpmiddle-rs.exe`.

### Configure the keyboard with CLI

It sends commands to the keyboard on its startup to set following configurations:

  * `--fn-lock` enables fn lock (disables without it).
  * `--sensitivity 1-9` sets TrackPoint sensitivity.

### Smooth scrolling (Experimental)

It disables native middle button on its startup to intercept all middle button events.
You can set scrolling controller by passing `--scroll <controller>` options to the program.

 * `classic`: It just bypasses middle button events. It would feel same with ThinkPad preferred scrolling.
 * `smooth`: It tries to smoothen discrete middle button events.

## How to install

Download `tpmiddle-rs.exe` and make a shortcut to Startup folder.

1. Remove ThinkPad TrackPoint Keyboard software and reboot.
1. Press **Win + R** key, type **shell:startup**, then select **OK**. Startup folder will show up.
1. Drag and drop downloaded `tpmiddle-rs.exe` to the folder while pressing **Alt** key.
1. Right click the created shortcut, click **Properties**.
1. Append some configuration flags such as `--fn-lock`, `--scroll smooth` to **Target**.
   Flags should be separated by a whitespace.
1. Press **Ok** button to save the shortcut.
1. You can activate it now by double clicking the shortcut without reboot.

## How to restore

It overrides the keyboard configurations on its startup.
You can restore it by removing `tpmiddle-rs.exe` and reinstalling ThinkPad TrackPoint Keyboard software.

1. Terminate `tpmiddle-rs.exe`
   1. Press **Alt + Ctrl + Delete**, click **Task Manager**
   1. Press **More Details**, click **Details** tab.
   1. Find `tpmiddle-rs.exe`, click **End Task**.
1. Remove `tpmiddle-rs.exe`
   1. Remove the shortcut from Startup folder.
   1. Remove `tpmiddle-rs.exe`.
1. Reinstall ThinkPad TrackPoint Keyboard softare.